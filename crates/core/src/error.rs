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

//! Errors: [`CoreError`] for this crate, and [`ErrorContext`] for adding
//! "while doing X" context to any error.
//!
//! Ported from nadir-core `error.rs` (the command-line variants). Adapted for
//! Tidepool: color errors carry the byte offset where parsing failed and what
//! was expected there, command-line errors carry the argument's position, the
//! command-line limits have their own variants, and [`ErrorContext`] and
//! [`WithContext`] are new.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;

/// Errors produced by `tidepool-core`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// A color string (see [`Color::parse`](crate::color::Color::parse)) is
    /// malformed.
    InvalidColor {
        /// Byte offset into the input where parsing failed.
        offset: usize,
        /// What the parser expected at `offset`, for example `"a hex digit"`.
        expected: &'static str,
    },
    /// A command-line option that takes a value was given without one.
    MissingOptionValue {
        /// The option, as the caller named it (for example `--secs`).
        option: String,
        /// The option's 1-based position on the command line, not counting
        /// the program name.
        position: usize,
    },
    /// The value of a command-line option could not be parsed.
    InvalidOptionValue {
        /// The option, as the caller named it (for example `--secs`).
        option: String,
        /// The value that was given.
        value: String,
        /// The option's 1-based position on the command line, not counting
        /// the program name.
        position: usize,
    },
    /// A command-line argument that nothing asked for: an unknown option, or
    /// a known one given twice or with a value it does not take.
    UnexpectedArgument {
        /// The argument as typed.
        argument: String,
        /// Its 1-based position on the command line, not counting the
        /// program name.
        position: usize,
    },
    /// The command line has more arguments than
    /// [`cli::Limits::max_args`](crate::cli::Limits::max_args).
    TooManyArguments {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// One argument is longer than
    /// [`cli::Limits::max_arg_len`](crate::cli::Limits::max_arg_len).
    ArgumentTooLong {
        /// The argument's 1-based position on the command line, not counting
        /// the program name.
        position: usize,
        /// Its length in bytes.
        length: usize,
        /// The limit that was exceeded, in bytes.
        limit: usize,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidColor { offset, expected } => {
                write!(f, "invalid color at byte {offset}: expected {expected}")
            }
            Self::MissingOptionValue { option, position } => {
                write!(f, "option `{option}` (argument {position}) needs a value")
            }
            Self::InvalidOptionValue {
                option,
                value,
                position,
            } => write!(
                f,
                "invalid value `{value}` for option `{option}` (argument {position})"
            ),
            Self::UnexpectedArgument { argument, position } => {
                write!(f, "unexpected argument `{argument}` (argument {position})")
            }
            Self::TooManyArguments { limit } => {
                write!(f, "too many command-line arguments: the limit is {limit}")
            }
            Self::ArgumentTooLong {
                position,
                length,
                limit,
            } => write!(
                f,
                "argument {position} is {length} bytes long: the limit is {limit} bytes"
            ),
        }
    }
}

impl Error for CoreError {}

/// Adds "while doing X" context to the error of a [`Result`].
///
/// ```
/// use std::io;
/// use tidepool_core::error::ErrorContext;
///
/// let failed: Result<(), io::Error> = Err(io::Error::other("disk full"));
/// let error = failed.context("saving settings.toml").unwrap_err();
/// assert_eq!(error.to_string(), "while saving settings.toml: disk full");
/// ```
pub trait ErrorContext<T, E> {
    /// Wraps the error, if any, in a [`WithContext`] that describes what was
    /// being done. Write `what` without the word "while": the error's
    /// message adds it.
    fn context(self, what: impl Into<Cow<'static, str>>) -> Result<T, WithContext<E>>;

    /// Like [`ErrorContext::context`], but builds the description only if
    /// there is an error.
    fn with_context<S, F>(self, what: F) -> Result<T, WithContext<E>>
    where
        S: Into<Cow<'static, str>>,
        F: FnOnce() -> S;
}

impl<T, E> ErrorContext<T, E> for Result<T, E> {
    fn context(self, what: impl Into<Cow<'static, str>>) -> Result<T, WithContext<E>> {
        self.map_err(|source| WithContext::new(what, source))
    }

    fn with_context<S, F>(self, what: F) -> Result<T, WithContext<E>>
    where
        S: Into<Cow<'static, str>>,
        F: FnOnce() -> S,
    {
        self.map_err(|source| WithContext::new(what(), source))
    }
}

/// An error together with a description of what was being done when it
/// happened. Made by [`ErrorContext`].
///
/// It displays as `while <what>: <source>`, and [`Error::source`] returns
/// the wrapped error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithContext<E> {
    what: Cow<'static, str>,
    source: E,
}

impl<E> WithContext<E> {
    /// Wraps `source` with the description `what` (without "while").
    pub fn new(what: impl Into<Cow<'static, str>>, source: E) -> Self {
        Self {
            what: what.into(),
            source,
        }
    }

    /// Returns what was being done, without "while".
    pub fn what(&self) -> &str {
        &self.what
    }

    /// Returns the wrapped error.
    pub fn inner(&self) -> &E {
        &self.source
    }

    /// Returns the wrapped error, dropping the context.
    pub fn into_inner(self) -> E {
        self.source
    }
}

impl<E: fmt::Display> fmt::Display for WithContext<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "while {}: {}", self.what, self.source)
    }
}

impl<E: Error + 'static> Error for WithContext<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn display_names_the_offending_argument_and_position() {
        let missing = CoreError::MissingOptionValue {
            option: "--secs".into(),
            position: 2,
        };
        assert_eq!(
            missing.to_string(),
            "option `--secs` (argument 2) needs a value"
        );
        let invalid = CoreError::InvalidOptionValue {
            option: "--secs".into(),
            value: "soon".into(),
            position: 1,
        };
        assert_eq!(
            invalid.to_string(),
            "invalid value `soon` for option `--secs` (argument 1)"
        );
        let unexpected = CoreError::UnexpectedArgument {
            argument: "--frobnicate".into(),
            position: 3,
        };
        assert_eq!(
            unexpected.to_string(),
            "unexpected argument `--frobnicate` (argument 3)"
        );
        assert_eq!(
            CoreError::TooManyArguments { limit: 10 }.to_string(),
            "too many command-line arguments: the limit is 10"
        );
        assert_eq!(
            CoreError::ArgumentTooLong {
                position: 4,
                length: 100,
                limit: 64
            }
            .to_string(),
            "argument 4 is 100 bytes long: the limit is 64 bytes"
        );
    }

    #[test]
    fn color_errors_carry_the_offset_and_expectation() {
        let error = CoreError::InvalidColor {
            offset: 3,
            expected: "a hex digit",
        };
        assert_eq!(
            error.to_string(),
            "invalid color at byte 3: expected a hex digit"
        );
    }

    #[test]
    fn context_wraps_errors_and_passes_values_through() {
        let ok: Result<u32, io::Error> = Ok(7);
        assert_eq!(ok.context("reading").ok(), Some(7));

        let failed: Result<u32, io::Error> = Err(io::Error::other("disk full"));
        let error = failed.context("saving settings.toml").unwrap_err();
        assert_eq!(error.to_string(), "while saving settings.toml: disk full");
        assert_eq!(error.what(), "saving settings.toml");
        assert_eq!(error.inner().kind(), io::ErrorKind::Other);
        let source = error.source().expect("the wrapped error is the source");
        assert_eq!(source.to_string(), "disk full");
        assert_eq!(error.into_inner().to_string(), "disk full");
    }

    #[test]
    fn with_context_is_lazy_and_contexts_nest() {
        let ok: Result<(), CoreError> = Ok(());
        let built = std::cell::Cell::new(false);
        let _ = ok.with_context(|| {
            built.set(true);
            "never shown"
        });
        assert!(!built.get(), "the description is built only on error");

        let failed: Result<(), CoreError> = Err(CoreError::TooManyArguments { limit: 1 });
        let inner = failed.with_context(|| format!("parsing {}", "argv"));
        let outer = inner.context("starting up").unwrap_err();
        assert_eq!(
            outer.to_string(),
            "while starting up: while parsing argv: too many command-line arguments: the limit is 1"
        );
        // The chain can be walked through `source`.
        let mut depth = 0;
        let mut current: Option<&dyn Error> = Some(&outer);
        while let Some(error) = current {
            depth += 1;
            current = error.source();
        }
        assert_eq!(depth, 3);
    }
}
