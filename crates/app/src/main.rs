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
//! Specification: `docs/01-architecture.md`.
//!
//! Today it opens the GUI's window and shows the first frame (M2.6) until
//! the window is closed. `--print-startup-time` prints, on standard output,
//! the time from the start of `main` to the return of the first present
//! (ADR-0045). The main loop and the shell come with M3.4.

use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tidepool_core::cli::Args;
use tidepool_gui::{Gui, GuiError};
use tidepool_platform::window::WindowConfig;

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

/// Opens the window and answers it until it is asked to close, writing
/// the startup time to `out` after the first present if `options` asks.
fn run(options: &Options, start: Instant, out: &mut dyn Write) -> Result<(), Error> {
    let mut gui = Gui::open(&WindowConfig::default()).map_err(Error::Gui)?;
    let mut first = true;
    loop {
        let turn = gui.turn(None).map_err(Error::Gui)?;
        if turn.presented && first {
            first = false;
            if options.print_startup_time {
                // If standard output is closed, there is nobody to tell,
                // and the window stays open.
                let _ = out
                    .write_all(startup_line(start.elapsed()).as_bytes())
                    .and_then(|()| out.flush());
            }
        }
        if turn.close_requested {
            return Ok(());
        }
    }
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
    use super::*;

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
