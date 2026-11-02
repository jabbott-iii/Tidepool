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

//! A minimal command-line argument parser for `tidepool`, `xtask` and the
//! reference agents in `agents/`.
//!
//! Ported from nadir-core `cli.rs`. Adapted for Tidepool: input is bounded
//! by [`Limits`], every error carries the argument's position, and an
//! option's value must be the argument right after it on the original
//! command line.
//!
//! It understands `--flag`, `--key value` (also `--key=value`), positional
//! arguments, and a bare `--` that makes everything after it positional.
//!
//! There is no schema. The caller pulls out what it understands, then calls
//! [`Args::finish`], which rejects whatever options are left:
//!
//! ```
//! use tidepool_core::cli::Args;
//!
//! let mut args = Args::new(["fuzz-smoke", "--secs", "60", "--verbose", "json"])?;
//! assert_eq!(args.subcommand().as_deref(), Some("fuzz-smoke"));
//! let secs: u64 = args.parsed("--secs")?.unwrap_or(10);
//! let verbose = args.flag("--verbose");
//! let targets = args.finish()?;
//! assert_eq!((secs, verbose, targets), (60, true, vec!["json".to_string()]));
//! # Ok::<(), tidepool_core::CoreError>(())
//! ```
//!
//! Because there is no schema, take every option before calling `finish`: an
//! option's value is only recognized as such when its option is asked for.
//! Parsing never panics, whatever the arguments.

use std::str::FromStr;

use crate::error::CoreError;

/// Bounds on the command line that [`Args`] accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most arguments accepted, not counting the program name.
    pub max_args: usize,
    /// The longest single argument accepted, in bytes.
    pub max_arg_len: usize,
}

impl Limits {
    /// 65,536 arguments of up to 128 KiB each.
    ///
    /// A deliberate bound, not an OS maximum: Linux can pass more arguments
    /// than this (its own cap is 128 KiB per argument and ARG_MAX in
    /// total), and macOS a longer single argument. No Tidepool tool needs
    /// more; a caller that does can pass its own limits to
    /// [`Args::with_limits`].
    pub const DEFAULT: Self = Self {
        max_args: 65_536,
        max_arg_len: 128 * 1024,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One argument and its 1-based position on the original command line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Arg {
    position: usize,
    text: String,
}

/// The arguments of one command-line invocation, consumed piece by piece.
#[derive(Clone, Debug)]
pub struct Args {
    /// Arguments before any bare `--`, minus those already taken.
    items: Vec<Arg>,
    /// Arguments after a bare `--`: always positional.
    trailing: Vec<Arg>,
}

impl Args {
    /// Creates a parser over `args`, which must not include the program
    /// name, with [`Limits::DEFAULT`].
    ///
    /// # Errors
    ///
    /// [`CoreError::TooManyArguments`] or [`CoreError::ArgumentTooLong`] if
    /// the arguments exceed the limits.
    pub fn new<I>(args: I) -> Result<Self, CoreError>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Self::with_limits(args, Limits::DEFAULT)
    }

    /// Creates a parser over `args`, which must not include the program
    /// name. Reads at most `limits.max_args + 1` items from `args`, so even
    /// an endless iterator is safe.
    ///
    /// # Errors
    ///
    /// [`CoreError::TooManyArguments`] or [`CoreError::ArgumentTooLong`] if
    /// the arguments exceed `limits`.
    pub fn with_limits<I>(args: I, limits: Limits) -> Result<Self, CoreError>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut items = Vec::new();
        for (index, arg) in args.into_iter().enumerate() {
            if index >= limits.max_args {
                return Err(CoreError::TooManyArguments {
                    limit: limits.max_args,
                });
            }
            let text: String = arg.into();
            let position = index + 1;
            if text.len() > limits.max_arg_len {
                return Err(CoreError::ArgumentTooLong {
                    position,
                    length: text.len(),
                    limit: limits.max_arg_len,
                });
            }
            items.push(Arg { position, text });
        }
        let trailing = match items.iter().position(|item| item.text == "--") {
            Some(separator) => {
                let trailing = items.split_off(separator + 1);
                items.pop();
                trailing
            }
            None => Vec::new(),
        };
        Ok(Self { items, trailing })
    }

    /// Creates a parser over the arguments of the current process, without
    /// the program name, with [`Limits::DEFAULT`].
    ///
    /// An argument that is not valid Unicode has its invalid parts replaced
    /// with `U+FFFD`.
    ///
    /// # Errors
    ///
    /// As for [`Args::new`].
    pub fn from_env() -> Result<Self, CoreError> {
        Self::new(
            std::env::args_os()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned()),
        )
    }

    /// Takes the first remaining argument if it is not an option, for
    /// `tool <command> ...` layouts. Call this before anything else.
    pub fn subcommand(&mut self) -> Option<String> {
        match self.items.first() {
            Some(first) if !is_option(&first.text) => Some(self.items.remove(0).text),
            _ => None,
        }
    }

    /// Takes the flag `name` (for example `"--bless"`) and returns whether
    /// it was present.
    pub fn flag(&mut self, name: &str) -> bool {
        match self.items.iter().position(|item| item.text == name) {
            Some(index) => {
                self.items.remove(index);
                true
            }
            None => false,
        }
    }

    /// Takes the option `name` (for example `"--target"`) and its value,
    /// given as `--target json` or `--target=json`.
    ///
    /// The value is the argument right after the option on the command
    /// line. A following argument that starts with `--` is not taken as the
    /// value; use the `=` form for such values.
    ///
    /// # Errors
    ///
    /// [`CoreError::MissingOptionValue`] if the option is present without a
    /// value. Returns `Ok(None)` if it is absent.
    pub fn value(&mut self, name: &str) -> Result<Option<String>, CoreError> {
        let is_match = |item: &Arg| item.text == name || inline_value(&item.text, name).is_some();
        let Some(index) = self.items.iter().position(is_match) else {
            return Ok(None);
        };
        let option = &self.items[index];
        if let Some(value) = inline_value(&option.text, name) {
            let value = value.to_string();
            self.items.remove(index);
            return Ok(Some(value));
        }
        // The next argument is the value only if it was adjacent on the
        // original command line: an argument some other call already took
        // must not be skipped over.
        let position = option.position;
        let has_value = self
            .items
            .get(index + 1)
            .is_some_and(|next| next.position == position + 1 && !next.text.starts_with("--"));
        if !has_value {
            return Err(CoreError::MissingOptionValue {
                option: name.to_string(),
                position,
            });
        }
        let value = self.items.remove(index + 1).text;
        self.items.remove(index);
        Ok(Some(value))
    }

    /// Takes the option `name` like [`Args::value`] and parses its value.
    ///
    /// # Errors
    ///
    /// As for [`Args::value`], and [`CoreError::InvalidOptionValue`] if the
    /// value does not parse as a `T`.
    pub fn parsed<T: FromStr>(&mut self, name: &str) -> Result<Option<T>, CoreError> {
        let position = self
            .items
            .iter()
            .find(|item| item.text == name || inline_value(&item.text, name).is_some())
            .map_or(0, |item| item.position);
        let Some(value) = self.value(name)? else {
            return Ok(None);
        };
        match value.parse() {
            Ok(parsed) => Ok(Some(parsed)),
            Err(_) => Err(CoreError::InvalidOptionValue {
                option: name.to_string(),
                value,
                position,
            }),
        }
    }

    /// Finishes parsing and returns the positional arguments in order.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnexpectedArgument`] if an option is left that nothing
    /// took: an unknown option, a repeated one, or a flag that was given a
    /// value.
    pub fn finish(self) -> Result<Vec<String>, CoreError> {
        if let Some(unexpected) = self.items.iter().find(|item| is_option(&item.text)) {
            return Err(CoreError::UnexpectedArgument {
                argument: unexpected.text.clone(),
                position: unexpected.position,
            });
        }
        Ok(self
            .items
            .into_iter()
            .chain(self.trailing)
            .map(|item| item.text)
            .collect())
    }
}

/// Returns the value of `item` if it has the form `name=value`.
fn inline_value<'a>(item: &'a str, name: &str) -> Option<&'a str> {
    item.strip_prefix(name)?.strip_prefix('=')
}

/// Anything that starts with `-` is an option, except a lone `-` (by
/// convention, standard input) and negative numbers.
fn is_option(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-') && arg.parse::<f64>().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check;

    fn args(items: &[&str]) -> Args {
        Args::new(items.iter().copied()).expect("within the default limits")
    }

    fn unexpected(argument: &str, position: usize) -> CoreError {
        CoreError::UnexpectedArgument {
            argument: argument.to_string(),
            position,
        }
    }

    fn missing(option: &str, position: usize) -> CoreError {
        CoreError::MissingOptionValue {
            option: option.to_string(),
            position,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn empty_command_line() {
        let mut args = args(&[]);
        assert_eq!(args.subcommand(), None);
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--target"), Ok(None));
        assert_eq!(args.parsed::<u32>("--secs"), Ok(None));
        assert_eq!(args.finish(), Ok(Vec::new()));
    }

    #[test]
    fn flags_values_and_positionals_in_any_order() {
        let mut args = args(&["golden", "scene_a", "--scale", "2", "--bless", "scene_b"]);
        assert_eq!(args.subcommand().as_deref(), Some("golden"));
        assert!(args.flag("--bless"));
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--scale"), Ok(Some("2".to_string())));
        assert_eq!(args.value("--scale"), Ok(None));
        assert_eq!(args.finish(), Ok(strings(&["scene_a", "scene_b"])));
    }

    #[test]
    fn an_option_value_may_start_with_a_single_dash() {
        // Only a following argument that starts with `--` is refused as the
        // value (see `Args::value`), so short-option-like and negative
        // values are taken as they are.
        let mut args = args(&[
            "--name",
            "-x",
            "--offset",
            "-5",
            "--pattern",
            "-",
            "--expr=-y",
        ]);
        assert_eq!(args.value("--name"), Ok(Some("-x".to_string())));
        assert_eq!(args.value("--offset"), Ok(Some("-5".to_string())));
        assert_eq!(args.value("--pattern"), Ok(Some("-".to_string())));
        assert_eq!(args.value("--expr"), Ok(Some("-y".to_string())));
        assert_eq!(args.finish(), Ok(Vec::new()));
    }

    #[test]
    fn equals_form() {
        let mut args = args(&["--target=json", "--secs=60", "--name=", "--expr=--x"]);
        assert_eq!(args.value("--target"), Ok(Some("json".to_string())));
        assert_eq!(args.parsed::<u64>("--secs"), Ok(Some(60)));
        assert_eq!(args.value("--name"), Ok(Some(String::new())));
        assert_eq!(args.value("--expr"), Ok(Some("--x".to_string())));
        assert_eq!(args.finish(), Ok(Vec::new()));
    }

    #[test]
    fn an_option_name_does_not_match_a_longer_option() {
        let mut args = args(&["--target-dir", "out", "--targets=3"]);
        assert_eq!(args.value("--target"), Ok(None));
        assert!(!args.flag("--target"));
        assert_eq!(args.value("--target-dir"), Ok(Some("out".to_string())));
        assert_eq!(args.finish(), Err(unexpected("--targets=3", 3)));
    }

    #[test]
    fn subcommand_is_only_the_first_argument() {
        let mut args = args(&["--verbose", "bench", "json"]);
        assert_eq!(args.subcommand(), None);
        assert!(args.flag("--verbose"));
        assert_eq!(args.subcommand().as_deref(), Some("bench"));
        assert_eq!(args.finish(), Ok(strings(&["json"])));
    }

    #[test]
    fn missing_values_are_errors_with_the_option_position() {
        // At the end of the line.
        assert_eq!(args(&["--secs"]).value("--secs"), Err(missing("--secs", 1)));
        // Followed by another option.
        assert_eq!(
            args(&["run", "--secs", "--target", "json"]).value("--secs"),
            Err(missing("--secs", 2))
        );
        assert_eq!(
            args(&["--secs"]).parsed::<u64>("--secs"),
            Err(missing("--secs", 1))
        );
    }

    #[test]
    fn a_value_must_be_adjacent_on_the_original_command_line() {
        // `-v` is taken first; `--secs` must not then claim `5`, which was
        // never next to it.
        let mut args = args(&["--secs", "-v", "5"]);
        assert!(args.flag("-v"));
        assert_eq!(args.value("--secs"), Err(missing("--secs", 1)));
    }

    #[test]
    fn unparsable_values_are_errors() {
        let mut args = args(&["--scale", "1.5", "--secs", "soon"]);
        let expected = CoreError::InvalidOptionValue {
            option: "--secs".to_string(),
            value: "soon".to_string(),
            position: 3,
        };
        assert_eq!(args.parsed::<u64>("--secs"), Err(expected));
        assert_eq!(args.parsed::<f32>("--scale"), Ok(Some(1.5)));
    }

    #[test]
    fn negative_numbers_are_values_and_positionals() {
        let mut args = args(&["tp", "--offset", "-5", "-12.5", "-", "-3"]);
        assert_eq!(args.subcommand().as_deref(), Some("tp"));
        assert_eq!(args.parsed::<i32>("--offset"), Ok(Some(-5)));
        assert_eq!(args.finish(), Ok(strings(&["-12.5", "-", "-3"])));
    }

    #[test]
    fn leftover_options_are_rejected_with_their_position() {
        // Unknown option.
        assert_eq!(
            args(&["--frobnicate"]).finish(),
            Err(unexpected("--frobnicate", 1))
        );
        assert_eq!(args(&["file", "-x"]).finish(), Err(unexpected("-x", 2)));

        // Given twice.
        let mut twice = args(&["--secs", "1", "--secs", "2"]);
        assert_eq!(twice.parsed::<u64>("--secs"), Ok(Some(1)));
        assert_eq!(twice.finish(), Err(unexpected("--secs", 3)));

        // A flag with a value it does not take.
        let mut valued_flag = args(&["--bless=yes"]);
        assert!(!valued_flag.flag("--bless"));
        assert_eq!(valued_flag.finish(), Err(unexpected("--bless=yes", 1)));
    }

    #[test]
    fn everything_after_a_bare_double_dash_is_positional() {
        let mut args = args(&["run", "--verbose", "--", "--bless", "--secs", "5", "--"]);
        assert_eq!(args.subcommand().as_deref(), Some("run"));
        assert!(args.flag("--verbose"));
        // Options after the separator are not options.
        assert!(!args.flag("--bless"));
        assert_eq!(args.value("--secs"), Ok(None));
        assert_eq!(
            args.finish(),
            Ok(strings(&["--bless", "--secs", "5", "--"]))
        );
    }

    #[test]
    fn a_value_cannot_be_taken_from_across_the_separator() {
        let mut args = args(&["--secs", "--", "5"]);
        assert_eq!(args.value("--secs"), Err(missing("--secs", 1)));
    }

    #[test]
    fn limits_bound_the_input() {
        let limits = Limits {
            max_args: 3,
            max_arg_len: 4,
        };
        assert!(Args::with_limits(["a", "b", "c"], limits).is_ok());
        assert_eq!(
            Args::with_limits(["a", "b", "c", "d"], limits).map(|_| ()),
            Err(CoreError::TooManyArguments { limit: 3 })
        );
        assert_eq!(
            Args::with_limits(["abcd", "abcde"], limits).map(|_| ()),
            Err(CoreError::ArgumentTooLong {
                position: 2,
                length: 5,
                limit: 4
            })
        );
        // An endless argument source stops at the limit.
        let endless = std::iter::repeat("x");
        assert_eq!(
            Args::with_limits(endless, limits).map(|_| ()),
            Err(CoreError::TooManyArguments { limit: 3 })
        );
        assert_eq!(Limits::default(), Limits::DEFAULT);
    }

    #[test]
    fn from_env_does_not_include_the_program_name() {
        // The test binary's own path must not show up as an argument.
        let program = std::env::args().next().expect("the test binary has a path");
        let args = Args::from_env().expect("the test runner's arguments fit");
        assert!(!args.items.iter().any(|item| item.text == program));
    }

    #[test]
    fn never_panics_on_arbitrary_arguments() {
        // Arguments built from option-like pieces, then every accessor in a
        // random order. Nothing may panic, and `finish` only ever returns
        // arguments that were given.
        const PIECES: [&str; 12] = [
            "--", "-", "--a", "--a=", "--a=1", "-a", "--b", "1", "-1", "x", "=", "--=",
        ];
        check(
            "cli never panics",
            2000,
            |rng| {
                let count = rng.range(0..12u32);
                let items: Vec<&str> = (0..count)
                    .map(|_| *rng.choose(&PIECES).expect("not empty"))
                    .collect();
                let calls: Vec<u32> = (0..6).map(|_| rng.range(0..5u32)).collect();
                (items, calls)
            },
            |(items, calls)| {
                let mut args = Args::new(items.iter().copied()).map_err(|e| e.to_string())?;
                for call in calls {
                    match call {
                        0 => {
                            let _ = args.subcommand();
                        }
                        1 => {
                            let _ = args.flag("--a");
                        }
                        2 => {
                            let _ = args.value("--a");
                        }
                        3 => {
                            let _ = args.parsed::<i32>("--b");
                        }
                        _ => {
                            let _ = args.value("--b");
                        }
                    }
                }
                if let Ok(rest) = args.finish()
                    && let Some(stranger) = rest.iter().find(|arg| !items.contains(&arg.as_str()))
                {
                    return Err(format!("`{stranger}` was never given"));
                }
                Ok(())
            },
        );
    }
}
