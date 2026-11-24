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

//! The crate's error, [`Error`].
//!
//! New in Tidepool; not ported. Nadir's `PlatformError` has some of the
//! same cases, with other names and messages.

use std::ffi::OsString;
use std::fmt;
use std::io;

/// What went wrong in `tidepool-platform`.
///
/// Every variant exists on every system, so that no `target_os` shows in
/// the API; each says on which systems it can happen. Non-exhaustive: each
/// task adds the cases of what it builds.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// This system does not have this yet: a window, the folders of
    /// [`paths`](crate::paths) and [`dirs`](crate::dirs), or the metadata
    /// of [`file`](crate::file). Linux has them; macOS gets them in M11.7
    /// and M11.8, and Windows in M12.1 and M12.2.
    Unsupported {
        /// What is missing, such as "window".
        what: &'static str,
        /// The system, as [`std::env::consts::OS`] names it.
        os: &'static str,
    },
    /// Linux: a folder lies in the home directory, which is not known:
    /// `HOME` is not an absolute path, or it is unset or empty and the
    /// user database gives no absolute path.
    NoHomeDirectory,
    /// Linux: the `DISPLAY` environment variable names no X server that the
    /// window may connect to. The window connects only to a local one, over
    /// a Unix socket: `DISPLAY` must be `:N`, `:N.S`, `unix:N` or
    /// `unix:N.S` (ADR-0039).
    Display {
        /// The variable's value, or `None` if it is not set. The message
        /// shows it escaped, as `Debug` does.
        value: Option<OsString>,
        /// Why it is refused, such as "names a host".
        reason: &'static str,
    },
    /// A shared library could not be loaded, such as `libX11.so.6` on a
    /// machine without a desktop.
    LibraryNotFound {
        /// The library's file name.
        library: &'static str,
        /// What the loader said.
        reason: String,
    },
    /// A shared library lacks functions that Tidepool calls.
    MissingSymbols {
        /// The library's file name.
        library: &'static str,
        /// Every function it lacks, in the order they are declared.
        symbols: Vec<&'static str>,
    },
    /// Linux: Xlib could not connect to the X server, which says no more
    /// than that: none may be running on that display, or it refused the
    /// connection.
    ConnectionFailed {
        /// The name the window gave Xlib, such as `unix/:0`.
        display: String,
    },
    /// Linux: the connection to the X server hung up.
    ///
    /// Xlib's own handler of a lost connection ends the process when it
    /// sees one, so this is reported only if the process lives on.
    ConnectionLost,
    /// A window is open on another thread. Every window of the process
    /// lives on one thread, the one that opened the first, until all of
    /// them are closed (`docs/03-platforms.md`).
    WrongThread,
    /// Linux: the screen has no visual of 24-bit TrueColor with red, green
    /// and blue in the bytes a BGRA8 frame puts them in.
    NoVisual {
        /// What the screen lacks.
        reason: String,
    },
    /// Linux: the X server refused a request made while the window was
    /// being opened. Requests made later are refused into the log instead.
    Refused {
        /// What went wrong: `BadWindow`, `BadMatch` and so on, as numbered
        /// in `X11/X.h`.
        error_code: u8,
        /// The major opcode of the request (`X11/Xproto.h`).
        request_code: u8,
        /// The minor opcode of the request, for an extension's.
        minor_code: u8,
    },
    /// A call into the system's window library failed, naming the call.
    CallFailed {
        /// The function that failed, such as `XCreateGC`.
        call: &'static str,
    },
    /// A system call failed.
    Io {
        /// The call that failed, such as `poll`.
        call: &'static str,
        /// What the system reported.
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { what, os } => write!(
                f,
                "Tidepool has no {what} on {os} yet: only on Linux (macOS comes in M11, \
                 Windows in M12)"
            ),
            Self::NoHomeDirectory => f.write_str(
                "the home directory is not known: HOME is not an absolute path, or it is unset \
                 or empty and the user database gives no absolute path",
            ),
            Self::Display { value, reason } => {
                match value {
                    Some(value) => write!(f, "DISPLAY={value:?} {reason}")?,
                    None => write!(f, "DISPLAY {reason}")?,
                }
                f.write_str(
                    ": the window connects only to a local X server, over a Unix socket \
                     (DISPLAY=:N or unix:N), never over TCP, so it does not run over ssh -X",
                )
            }
            Self::LibraryNotFound { library, reason } => {
                write!(f, "cannot load {library}: {reason}")
            }
            Self::MissingSymbols { library, symbols } => {
                let noun = if symbols.len() == 1 {
                    "function"
                } else {
                    "functions"
                };
                write!(f, "{library} lacks the {noun} {}", symbols.join(", "))
            }
            Self::ConnectionFailed { display } => {
                write!(f, "cannot connect to the X server at {display}")
            }
            Self::ConnectionLost => f.write_str("the connection to the X server was lost"),
            Self::WrongThread => f.write_str(
                "another thread has a window open: every window lives on the thread that \
                 opened the first, until all are closed",
            ),
            Self::NoVisual { reason } => {
                write!(f, "the X server has no visual to draw in: {reason}")
            }
            Self::Refused {
                error_code,
                request_code,
                minor_code,
            } => write!(
                f,
                "the X server refused a request while opening the window: error {error_code}, \
                 request {request_code}.{minor_code}"
            ),
            Self::CallFailed { call } => write!(f, "{call} failed"),
            Self::Io { call, source } => write!(f, "{call}: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Unsupported { .. }
            | Self::NoHomeDirectory
            | Self::Display { .. }
            | Self::LibraryNotFound { .. }
            | Self::MissingSymbols { .. }
            | Self::ConnectionFailed { .. }
            | Self::ConnectionLost
            | Self::WrongThread
            | Self::NoVisual { .. }
            | Self::Refused { .. }
            | Self::CallFailed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn a_refused_display_is_shown_escaped_with_the_way_out() {
        let error = Error::Display {
            value: Some("localhost:10.0".into()),
            reason: "names a host",
        };
        let message = error.to_string();
        assert!(
            message.starts_with("DISPLAY=\"localhost:10.0\" names a host: "),
            "{message}"
        );
        assert!(message.contains("DISPLAY=:N or unix:N"), "{message}");

        // Control characters are escaped. (Bytes that are not UTF-8 are
        // tested with the display's rules, on Linux.)
        let error = Error::Display {
            value: Some(OsString::from(":\u{7}\n")),
            reason: "has a display number that is not a decimal number",
        };
        let message = error.to_string();
        assert!(
            message.starts_with(r#"DISPLAY=":\u{7}\n" has"#),
            "{message}"
        );
        assert!(!message.contains(['\n', '\u{7}']), "{message}");

        let unset = Error::Display {
            value: None,
            reason: "is not set",
        };
        assert!(unset.to_string().starts_with("DISPLAY is not set: "));
    }

    #[test]
    fn messages_name_what_failed() {
        let cases = [
            (
                Error::Unsupported {
                    what: "window",
                    os: "macos",
                },
                "Tidepool has no window on macos yet",
            ),
            (
                Error::Unsupported {
                    what: "user folders",
                    os: "windows",
                },
                "Tidepool has no user folders on windows yet",
            ),
            (Error::NoHomeDirectory, "the home directory is not known"),
            (
                Error::LibraryNotFound {
                    library: "libX11.so.6",
                    reason: "not found".to_string(),
                },
                "cannot load libX11.so.6: not found",
            ),
            (
                Error::MissingSymbols {
                    library: "libX11.so.6",
                    symbols: vec!["XOpenIM"],
                },
                "libX11.so.6 lacks the function XOpenIM",
            ),
            (
                Error::MissingSymbols {
                    library: "libX11.so.6",
                    symbols: vec!["XOpenIM", "XCreateIC"],
                },
                "libX11.so.6 lacks the functions XOpenIM, XCreateIC",
            ),
            (
                Error::ConnectionFailed {
                    display: "unix/:7".to_string(),
                },
                "cannot connect to the X server at unix/:7",
            ),
            (
                Error::Refused {
                    error_code: 8,
                    request_code: 1,
                    minor_code: 0,
                },
                "error 8, request 1.0",
            ),
            (Error::CallFailed { call: "XCreateGC" }, "XCreateGC failed"),
        ];
        for (error, expected) in cases {
            let message = error.to_string();
            assert!(message.contains(expected), "{message}");
            assert!(error.source().is_none(), "{message}");
        }
    }

    #[test]
    fn a_failed_system_call_is_the_source() {
        let error = Error::Io {
            call: "poll",
            source: io::Error::from(io::ErrorKind::InvalidInput),
        };
        assert!(error.to_string().starts_with("poll: "));
        let source = error.source().map(ToString::to_string);
        assert_eq!(
            source,
            Some(io::Error::from(io::ErrorKind::InvalidInput).to_string())
        );
    }
}
