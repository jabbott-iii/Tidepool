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

//! The error type for `xtask`.
//!
//! Ported from nadir xtask `error.rs`. Adapted for Tidepool: errors for the
//! dependency-matrix document and for commands that are not implemented
//! yet. Nadir's file-writing variant, [`Error::Write`], is ported for `ucd`
//! (roadmap M1.1); its fuzzing variants stay in Nadir until their roadmap
//! task.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::walk;

/// Why a command could not do its job.
///
/// A check that ran and found problems is not an error: it returns a
/// [`Report`](crate::check::Report) with violations.
#[derive(Debug)]
pub(crate) enum Error {
    /// The command line was not understood.
    Usage(String),
    /// A command named in `AGENTS.md` whose roadmap task is not done yet.
    NotImplemented {
        /// The command.
        command: &'static str,
        /// The roadmap task that implements it.
        task: &'static str,
    },
    /// A file or directory could not be read.
    Read {
        /// The file or directory.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
    /// A command could not be started.
    Spawn {
        /// The command line.
        command: String,
        /// What the OS reported.
        source: io::Error,
    },
    /// A command ran and reported failure.
    CommandFailed {
        /// The command line.
        command: String,
        /// How the command ended.
        status: ExitStatus,
        /// What the command wrote to standard error, if that was captured,
        /// less the source lines cargo quotes, which could hold a secret
        /// (`cargo::without_quoted_source`, AGENTS.md rule 7).
        stderr: String,
    },
    /// A command printed something this tool does not understand.
    UnexpectedOutput {
        /// The command line.
        command: String,
        /// What is wrong with the output.
        detail: String,
    },
    /// A document this tool reads as data (the dependency matrix) is not in
    /// the expected shape.
    Doc {
        /// The document, relative to the repository root.
        path: String,
        /// The line, counted from 1, or 0 for the document as a whole.
        line: usize,
        /// What is wrong.
        detail: String,
    },
    /// Writing to standard output failed.
    Output(io::Error),
    /// A file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// What the OS reported.
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::NotImplemented { command, task } => {
                write!(f, "`{command}` is not yet implemented ({task})")
            }
            Self::Read { path, source } => {
                write!(f, "cannot read `{}`: {source}", walk::shown(path))
            }
            Self::Spawn { command, source } => write!(f, "cannot run `{command}`: {source}"),
            Self::CommandFailed {
                command,
                status,
                stderr,
            } => {
                write!(f, "`{command}` failed ({status})")?;
                match stderr.trim_end() {
                    "" => Ok(()),
                    stderr => write!(f, ":\n{stderr}"),
                }
            }
            Self::UnexpectedOutput { command, detail } => {
                write!(f, "unexpected output from `{command}`: {detail}")
            }
            Self::Doc {
                path,
                line: 0,
                detail,
            } => write!(f, "{path}: {detail}"),
            Self::Doc { path, line, detail } => write!(f, "{path}:{line}: {detail}"),
            Self::Output(source) => write!(f, "cannot write to standard output: {source}"),
            Self::Write { path, source } => {
                write!(f, "cannot write `{}`: {source}", walk::shown(path))
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::Spawn { source, .. }
            | Self::Write { source, .. }
            | Self::Output(source) => Some(source),
            Self::Usage(_)
            | Self::NotImplemented { .. }
            | Self::CommandFailed { .. }
            | Self::UnexpectedOutput { .. }
            | Self::Doc { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn display_names_the_file_command_or_line() {
        let read = Error::Read {
            path: PathBuf::from("xtask/deps-allowlist.txt"),
            source: io::Error::from(io::ErrorKind::NotFound),
        };
        let message = read.to_string();
        assert!(
            message.starts_with("cannot read `xtask/deps-allowlist.txt`: "),
            "{message}"
        );
        assert!(read.source().is_some());

        let unexpected = Error::UnexpectedOutput {
            command: "cargo metadata".to_string(),
            detail: "`packages` is missing".to_string(),
        };
        assert_eq!(
            unexpected.to_string(),
            "unexpected output from `cargo metadata`: `packages` is missing"
        );
        assert!(unexpected.source().is_none());

        let doc = Error::Doc {
            path: "docs/01-architecture.md".to_string(),
            line: 12,
            detail: "bad row".to_string(),
        };
        assert_eq!(doc.to_string(), "docs/01-architecture.md:12: bad row");
        let whole = Error::Doc {
            path: "docs/01-architecture.md".to_string(),
            line: 0,
            detail: "no matrix".to_string(),
        };
        assert_eq!(whole.to_string(), "docs/01-architecture.md: no matrix");

        let write = Error::Write {
            path: PathBuf::from("crates/text/src/pos/grapheme/tables.rs"),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        };
        let message = write.to_string();
        assert!(
            message.starts_with("cannot write `crates/text/src/pos/grapheme/tables.rs`: "),
            "{message}"
        );
        assert!(write.source().is_some());

        let later = Error::NotImplemented {
            command: "tools",
            task: "roadmap M5.1",
        };
        assert_eq!(
            later.to_string(),
            "`tools` is not yet implemented (roadmap M5.1)"
        );
    }
}
