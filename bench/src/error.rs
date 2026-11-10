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

//! The error type of `tidepool-bench`.

use std::fmt;
use std::io;
use std::path::PathBuf;

use tidepool_text::TextError;

/// Why a run, or one suite, could not do its job. Every one of them ends a
/// run with exit status 1 (ADR-0035); a suite that got slower is not an
/// error.
#[derive(Debug)]
#[non_exhaustive]
pub enum BenchError {
    /// The command line was not understood.
    Usage(String),
    /// The filter matched no suite.
    NoSuiteMatches {
        /// The filter, as given.
        filter: String,
    },
    /// A text operation that a suite's setup or routine runs failed.
    Text(TextError),
    /// A suite's own check of what its routine does failed.
    Check(String),
    /// One suite failed: its setup or check returned an error, it
    /// panicked, or it measured no routine or more than one.
    Suite {
        /// The suite's name.
        name: String,
        /// What went wrong.
        detail: String,
    },
    /// Suites failed; the others ran and were reported.
    SuitesFailed {
        /// The names of the suites that failed, in the order they ran.
        names: Vec<String>,
    },
    /// The baseline file exists but could not be read.
    Read {
        /// The file.
        path: PathBuf,
        /// What the OS reported, or why the file was not read.
        source: io::Error,
    },
    /// The baseline file is not one this build reads: not JSON, not the
    /// shape of a baseline, or another format version.
    Parse {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        detail: String,
    },
    /// The baseline file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        detail: String,
    },
    /// `--save` would merge into a baseline recorded on another machine.
    OtherMachine {
        /// The file.
        path: PathBuf,
        /// How the file's machine differs from this one.
        detail: String,
    },
    /// Writing the report to standard output failed.
    Output(io::Error),
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::NoSuiteMatches { filter } => write!(
                f,
                "no suite's name contains `{}`; `--help` lists the suites",
                printable(filter)
            ),
            Self::Text(error) => write!(f, "{error}"),
            Self::Check(detail) => f.write_str(detail),
            Self::Suite { name, detail } => write!(f, "suite `{name}` failed: {detail}"),
            Self::SuitesFailed { names } => {
                let count = names.len();
                let suites = if count == 1 { "suite" } else { "suites" };
                write!(f, "{count} {suites} failed: {}", names.join(", "))
            }
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", shown(path))
            }
            Self::Parse { path, detail } => {
                write!(
                    f,
                    "`{}` is not a baseline this build reads: {detail}",
                    shown(path)
                )
            }
            Self::Write { path, detail } => {
                write!(f, "cannot write `{}`: {detail}", shown(path))
            }
            Self::OtherMachine { path, detail } => write!(
                f,
                "refusing to merge into `{}`: {detail}; one baseline never mixes machines, so move \
                 the file aside to record one on this machine",
                shown(path)
            ),
            Self::Output(error) => write!(f, "cannot write the report: {error}"),
        }
    }
}

impl std::error::Error for BenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Text(error) => Some(error),
            Self::Read { source, .. } => Some(source),
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TextError> for BenchError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}

/// `text` with every control character escaped (`\u{1b}`), so that a name
/// read from a file or the command line cannot move the terminal's cursor
/// or change its colours when it is shown.
pub fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// A path for a message, through [`printable`].
pub(crate) fn shown(path: &std::path::Path) -> String {
    printable(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_escaped() {
        assert_eq!(printable("a\u{1b}[2Kb\r\n"), "a\\u{1b}[2Kb\\u{d}\\u{a}");
        assert_eq!(printable("MSI Pulse GL66 — é"), "MSI Pulse GL66 — é");
    }

    #[test]
    fn messages_name_what_failed() {
        assert_eq!(
            BenchError::NoSuiteMatches {
                filter: "nosuch\u{7}".to_string()
            }
            .to_string(),
            "no suite's name contains `nosuch\\u{7}`; `--help` lists the suites"
        );
        assert_eq!(
            BenchError::SuitesFailed {
                names: vec!["a/b".to_string()]
            }
            .to_string(),
            "1 suite failed: a/b"
        );
        assert_eq!(
            BenchError::SuitesFailed {
                names: vec!["a".to_string(), "b".to_string()]
            }
            .to_string(),
            "2 suites failed: a, b"
        );
    }
}
