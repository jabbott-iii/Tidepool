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

//! The baseline, `bench/baseline.json` (ADR-0035): the machine it was
//! recorded on and each suite's median, p95 and sample count.
//!
//! It is JSON read and written with [`tidepool_codec::json`], pretty and
//! with every object's keys sorted, ending in a line break:
//!
//! ```json
//! {
//!   "machine": {
//!     "arch": "x86_64",
//!     "cpu": "11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz",
//!     "name": "the name given to --machine",
//!     "os": "linux",
//!     "threads": 16
//!   },
//!   "suites": {
//!     "text/rope/insert/400kb": {
//!       "median_ns": 9000,
//!       "p95_ns": 9500,
//!       "samples": 400
//!     }
//!   },
//!   "version": 1
//! }
//! ```
//!
//! `version` is the format's ([`VERSION`]); a file of another version is
//! refused, not guessed at. The machine is the name the user gave and the
//! [`Facts`] the harness reads itself. Reading is strict: a member missing,
//! unknown or given twice, a value of the wrong type, or a count that is
//! not a whole number fails with what and where.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use tidepool_codec::Limits;
use tidepool_codec::json::{self, Dialect, Value};
use tidepool_text::encoding::Format;
use tidepool_text::{Rope, TextError};

use crate::BenchError;
use crate::error::printable;

/// The version of the file's format that this build reads and writes.
pub const VERSION: u64 = 1;

/// The file's name, in the crate's directory.
pub const FILE_NAME: &str = "baseline.json";

/// The largest baseline read, in bytes: far more than any number of suites
/// needs.
pub const MAX_BYTES: usize = 1 << 20;

/// The largest `/proc/cpuinfo` read, in bytes. The first `model name` line
/// is near the start.
const MAX_CPUINFO_BYTES: u64 = 1 << 20;

/// `bench/baseline.json`, found from the crate's own directory, so it is
/// the same file whatever the working directory.
pub fn default_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE_NAME)
}

/// What the harness reads about the machine it runs on. A baseline is
/// compared with a run only where these are equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The OS, as [`std::env::consts::OS`] names it.
    pub os: String,
    /// The architecture, as [`std::env::consts::ARCH`] names it.
    pub arch: String,
    /// The threads [`std::thread::available_parallelism`] reports, or 0
    /// when it cannot tell.
    pub threads: u64,
    /// The CPU's model, from the first `model name` line of
    /// `/proc/cpuinfo` ([`cpu_model`]); empty where that cannot be read, as
    /// on macOS and Windows.
    pub cpu: String,
}

impl Facts {
    /// The facts of this machine.
    pub fn detect() -> Self {
        let threads = std::thread::available_parallelism().map_or(0, |threads| {
            u64::try_from(threads.get()).unwrap_or(u64::MAX)
        });
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            threads,
            cpu: read_cpu_model(Path::new("/proc/cpuinfo")),
        }
    }

    /// The facts in a line, for messages: `linux x86_64, 16 threads, CPU
    /// "…"`.
    pub fn describe(&self) -> String {
        let cpu = if self.cpu.is_empty() {
            "CPU model unknown".to_string()
        } else {
            format!("CPU \"{}\"", printable(&self.cpu))
        };
        format!(
            "{} {}, {} threads, {cpu}",
            printable(&self.os),
            printable(&self.arch),
            self.threads
        )
    }
}

/// The CPU model in `/proc/cpuinfo`'s text: the value of its first line
/// whose key is `model name`, trimmed; empty when there is none.
pub fn cpu_model(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim() == "model name")
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default()
}

/// [`cpu_model`] of the file at `path`, read as a plain file; empty when it
/// cannot be read or is not UTF-8.
fn read_cpu_model(path: &Path) -> String {
    let mut text = String::new();
    match File::open(path).and_then(|file| file.take(MAX_CPUINFO_BYTES).read_to_string(&mut text)) {
        Ok(_) => cpu_model(&text),
        Err(_) => String::new(),
    }
}

/// The machine a baseline was recorded on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// The name the user gave with `--machine`.
    pub name: String,
    /// What the harness read about it.
    pub facts: Facts,
}

/// One suite's numbers in a baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The median per iteration, in whole nanoseconds.
    pub median_ns: u64,
    /// The p95 per iteration, by nearest rank, in whole nanoseconds.
    pub p95_ns: u64,
    /// How many samples they come from.
    pub samples: u64,
}

impl From<crate::Summary> for Entry {
    fn from(summary: crate::Summary) -> Self {
        Self {
            median_ns: summary.median_ns,
            p95_ns: summary.p95_ns,
            samples: u64::try_from(summary.samples).unwrap_or(u64::MAX),
        }
    }
}

/// A baseline: the machine and each suite's numbers, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baseline {
    /// Where it was recorded.
    pub machine: Machine,
    /// The suites, sorted by name.
    pub suites: BTreeMap<String, Entry>,
}

impl Baseline {
    /// Reads the baseline at `path`; `Ok(None)` when there is no file.
    ///
    /// # Errors
    ///
    /// [`BenchError::Read`] if it exists but cannot be read or is over
    /// [`MAX_BYTES`]; [`BenchError::Parse`] if it is not a baseline of
    /// [`VERSION`] ([`Baseline::parse`]).
    pub fn read(path: &Path) -> Result<Option<Self>, BenchError> {
        let read_error = |source| BenchError::Read {
            path: path.to_path_buf(),
            source,
        };
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(read_error(error)),
        };
        let mut bytes = Vec::new();
        let limit = u64::try_from(MAX_BYTES).unwrap_or(u64::MAX);
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        if bytes.len() > MAX_BYTES {
            return Err(read_error(io::Error::other(format!(
                "it is over {MAX_BYTES} bytes"
            ))));
        }
        Self::parse(&bytes)
            .map(Some)
            .map_err(|detail| BenchError::Parse {
                path: path.to_path_buf(),
                detail,
            })
    }

    /// Reads a baseline from the bytes of its file.
    ///
    /// # Errors
    ///
    /// What is wrong, as a phrase: the JSON error, or the member that is
    /// missing, unknown, given twice or of the wrong type, or a version
    /// other than [`VERSION`].
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let limits = Limits {
            max_input_bytes: MAX_BYTES,
            ..Limits::DEFAULT
        };
        let value = json::parse_bytes(bytes, Dialect::Json, limits).map_err(|e| e.to_string())?;
        let top = Members::of(&value, "the file", &["machine", "suites", "version"])?;
        let version = top.count("version")?;
        if version != VERSION {
            return Err(format!(
                "its format version is {version}, and this build reads only version {VERSION}"
            ));
        }
        let machine = Members::of(
            top.get("machine")?,
            "`machine`",
            &["arch", "cpu", "name", "os", "threads"],
        )?;
        let machine = Machine {
            name: machine.text("name")?,
            facts: Facts {
                os: machine.text("os")?,
                arch: machine.text("arch")?,
                threads: machine.count("threads")?,
                cpu: machine.text("cpu")?,
            },
        };
        let suites_value = top.get("suites")?;
        let Some(members) = suites_value.as_object() else {
            return Err("`suites` is not an object".to_string());
        };
        let mut suites = BTreeMap::new();
        for (name, entry) in members {
            let what = format!("suite `{}`", printable(name));
            let fields = Members::of(entry, &what, &["median_ns", "p95_ns", "samples"])?;
            let entry = Entry {
                median_ns: fields.count("median_ns")?,
                p95_ns: fields.count("p95_ns")?,
                samples: fields.count("samples")?,
            };
            if suites.insert(name.clone(), entry).is_some() {
                return Err(format!("{what} is given twice"));
            }
        }
        Ok(Self { machine, suites })
    }

    /// The file's text: pretty JSON with sorted keys, and a line break at
    /// the end.
    pub fn to_json(&self) -> String {
        let facts = &self.machine.facts;
        let machine = Value::Object(vec![
            ("arch".to_string(), Value::from(facts.arch.as_str())),
            ("cpu".to_string(), Value::from(facts.cpu.as_str())),
            ("name".to_string(), Value::from(self.machine.name.as_str())),
            ("os".to_string(), Value::from(facts.os.as_str())),
            ("threads".to_string(), Value::from(facts.threads)),
        ]);
        // A `BTreeMap` iterates in sorted order.
        let suites = self
            .suites
            .iter()
            .map(|(name, entry)| {
                let fields = Value::Object(vec![
                    ("median_ns".to_string(), Value::from(entry.median_ns)),
                    ("p95_ns".to_string(), Value::from(entry.p95_ns)),
                    ("samples".to_string(), Value::from(entry.samples)),
                ]);
                (name.clone(), fields)
            })
            .collect();
        let file = Value::Object(vec![
            ("machine".to_string(), machine),
            ("suites".to_string(), Value::Object(suites)),
            ("version".to_string(), Value::from(VERSION)),
        ]);
        let mut text = json::to_string_pretty(&file);
        text.push('\n');
        text
    }

    /// Writes the file at `path` atomically, through
    /// [`tidepool_text::file::save`]: a temporary file beside it, synced,
    /// renamed over it, and then the directory synced.
    ///
    /// # Errors
    ///
    /// [`BenchError::Write`] if the save fails. A failure before the rename
    /// leaves the old file as it was. A failure to sync the directory
    /// ([`FileStep::SyncDirectory`](tidepool_text::FileStep::SyncDirectory))
    /// comes after it: the file already holds the new baseline, which may
    /// not survive a crash, and the message says so.
    pub fn write(&self, path: &Path) -> Result<(), BenchError> {
        tidepool_text::file::save(path, &Rope::from(self.to_json()), Format::UTF8)
            .map_err(|error| write_error(path, &error))
    }

    /// The baseline `--save` writes: `existing`'s suites whose names are
    /// still in `known`, with `measured` written over them, all on
    /// `machine`.
    ///
    /// # Errors
    ///
    /// How the machines differ, if `existing` was recorded on another
    /// machine: another name, or other facts.
    pub fn merge(
        existing: Option<Baseline>,
        machine: Machine,
        measured: impl IntoIterator<Item = (String, Entry)>,
        known: &[&str],
    ) -> Result<Baseline, String> {
        let mut suites = BTreeMap::new();
        if let Some(existing) = existing {
            existing.check_machine(&machine)?;
            suites = existing.suites;
            suites.retain(|name, _| known.contains(&name.as_str()));
        }
        suites.extend(measured);
        Ok(Baseline { machine, suites })
    }

    /// Whether `--save` on `machine` may merge into this baseline: only if
    /// it was recorded on a machine of the same name and facts.
    ///
    /// # Errors
    ///
    /// How the machines differ: another name, or other facts.
    pub fn check_machine(&self, machine: &Machine) -> Result<(), String> {
        let recorded = &self.machine;
        if recorded.name != machine.name {
            return Err(format!(
                "it was recorded on `{}`, not on `{}`",
                printable(&recorded.name),
                printable(&machine.name)
            ));
        }
        if recorded.facts != machine.facts {
            return Err(format!(
                "it was recorded on `{}` as {}, and this machine is {}",
                printable(&recorded.name),
                recorded.facts.describe(),
                machine.facts.describe()
            ));
        }
        Ok(())
    }
}

/// The error of a failed save of `path`. `text`'s message names the step,
/// and for
/// [`FileStep::SyncDirectory`](tidepool_text::FileStep::SyncDirectory)
/// says that the file already holds the new text.
fn write_error(path: &Path, error: &TextError) -> BenchError {
    BenchError::Write {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}

/// The members of a JSON object that must have exactly the keys given, once
/// each.
struct Members<'a> {
    what: &'a str,
    members: &'a [(String, Value)],
}

impl<'a> Members<'a> {
    /// `value`'s members, if it is an object with exactly `keys`, each once.
    /// `what` names it in errors.
    fn of(value: &'a Value, what: &'a str, keys: &[&str]) -> Result<Self, String> {
        let Some(members) = value.as_object() else {
            return Err(format!("{what} is not an object"));
        };
        for (index, (key, _)) in members.iter().enumerate() {
            if !keys.contains(&key.as_str()) {
                return Err(format!("{what} has an unknown member `{}`", printable(key)));
            }
            if members[..index].iter().any(|(other, _)| other == key) {
                return Err(format!("{what} has `{key}` twice"));
            }
        }
        if let Some(missing) = keys
            .iter()
            .find(|key| !members.iter().any(|(name, _)| name == *key))
        {
            return Err(format!("{what} has no `{missing}`"));
        }
        Ok(Self { what, members })
    }

    fn get(&self, key: &str) -> Result<&'a Value, String> {
        self.members
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .ok_or_else(|| format!("{} has no `{key}`", self.what))
    }

    fn text(&self, key: &str) -> Result<String, String> {
        self.get(key)?
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("`{key}` of {} is not a string", self.what))
    }

    fn count(&self, key: &str) -> Result<u64, String> {
        self.get(key)?.as_u64().ok_or_else(|| {
            format!(
                "`{key}` of {} is not a whole number from 0 to 2^64 - 1",
                self.what
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::facts;
    use tidepool_text::FileStep;

    fn machine(name: &str) -> Machine {
        Machine {
            name: name.to_string(),
            facts: facts(),
        }
    }

    fn entry(median_ns: u64) -> Entry {
        Entry {
            median_ns,
            p95_ns: median_ns + 5,
            samples: 10,
        }
    }

    fn sample() -> Baseline {
        Baseline {
            machine: machine("laptop"),
            suites: [
                ("text/b".to_string(), entry(200)),
                ("text/a".to_string(), entry(100)),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn the_file_is_pretty_sorted_json_that_reads_back() {
        let baseline = sample();
        let text = baseline.to_json();
        assert_eq!(
            text,
            "{\n  \"machine\": {\n    \"arch\": \"x86_64\",\n    \"cpu\": \"11th Gen Intel(R) \
             Core(TM) i7-11800H @ 2.30GHz\",\n    \"name\": \"laptop\",\n    \"os\": \"linux\",\n    \
             \"threads\": 16\n  },\n  \"suites\": {\n    \"text/a\": {\n      \"median_ns\": 100,\n      \
             \"p95_ns\": 105,\n      \"samples\": 10\n    },\n    \"text/b\": {\n      \"median_ns\": \
             200,\n      \"p95_ns\": 205,\n      \"samples\": 10\n    }\n  },\n  \"version\": 1\n}\n"
        );
        assert_eq!(Baseline::parse(text.as_bytes()), Ok(baseline));
        // No suites is still a baseline.
        let empty = Baseline {
            machine: machine("x"),
            suites: BTreeMap::new(),
        };
        assert!(empty.to_json().contains("\"suites\": {}"));
        assert_eq!(Baseline::parse(empty.to_json().as_bytes()), Ok(empty));
    }

    #[test]
    fn a_malformed_file_is_refused_with_what_is_wrong() {
        let good = sample().to_json();
        let edited = |from: &str, to: &str| {
            assert!(good.contains(from), "{from}");
            Baseline::parse(good.replacen(from, to, 1).as_bytes()).unwrap_err()
        };
        assert_eq!(
            edited("\"version\": 1", "\"version\": 2"),
            "its format version is 2, and this build reads only version 1"
        );
        assert_eq!(
            edited("\"version\": 1", "\"version\": \"1\""),
            "`version` of the file is not a whole number from 0 to 2^64 - 1"
        );
        assert_eq!(
            edited(",\n  \"version\": 1", ""),
            "the file has no `version`"
        );
        assert_eq!(
            edited("\"version\": 1", "\"version\": 1, \"extra\": 0"),
            "the file has an unknown member `extra`"
        );
        assert_eq!(
            edited("\"version\": 1", "\"version\": 1, \"version\": 1"),
            "the file has `version` twice"
        );
        assert_eq!(
            edited("\"threads\": 16", "\"threads\": -16"),
            "`threads` of `machine` is not a whole number from 0 to 2^64 - 1"
        );
        assert_eq!(
            edited("\"name\": \"laptop\"", "\"name\": null"),
            "`name` of `machine` is not a string"
        );
        assert_eq!(
            edited("\"median_ns\": 100", "\"median_ns\": 100.5"),
            "`median_ns` of suite `text/a` is not a whole number from 0 to 2^64 - 1"
        );
        assert_eq!(
            edited(
                ",\n      \"samples\": 10\n    },\n    \"text/b\"",
                "\n    },\n    \"text/b\""
            ),
            "suite `text/a` has no `samples`"
        );
        assert_eq!(
            edited("\"text/b\"", "\"text/a\""),
            "suite `text/a` is given twice"
        );
        // A name from the file reaches the message escaped.
        assert_eq!(
            edited("\"version\": 1", "\"version\": 1, \"\\u001b[2K\": 0"),
            "the file has an unknown member `\\u{1b}[2K`"
        );
        let no_suites = Baseline {
            machine: machine("laptop"),
            suites: BTreeMap::new(),
        }
        .to_json();
        assert_eq!(
            Baseline::parse(
                no_suites
                    .replace("\"suites\": {}", "\"suites\": []")
                    .as_bytes()
            )
            .unwrap_err(),
            "`suites` is not an object"
        );
        assert_eq!(
            Baseline::parse(
                no_suites
                    .replace("\"suites\": {}", "\"suites\": {\"text/a\": 3}")
                    .as_bytes()
            )
            .unwrap_err(),
            "suite `text/a` is not an object"
        );
        assert_eq!(
            Baseline::parse(b"[]").unwrap_err(),
            "the file is not an object"
        );
        assert_eq!(
            Baseline::parse(br#"{"machine": {}, "suites": {}, "version": 1}"#).unwrap_err(),
            "`machine` has no `arch`"
        );
        assert_eq!(
            Baseline::parse(
                good.replacen("\"suites\": {", "\"suites\": [", 1)
                    .replacen("\n  },\n  \"version\"", "\n  ],\n  \"version\"", 1)
                    .as_bytes()
            )
            .map_err(|e| e.starts_with("JSON error at ")),
            Err(true),
            "a JSON error is passed on"
        );
        for bytes in [&b""[..], b"\xff", b"{} {}"] {
            let error = Baseline::parse(bytes).unwrap_err();
            assert!(error.starts_with("JSON error at "), "{error}");
        }
    }

    #[test]
    fn reading_a_missing_file_is_no_baseline_and_a_bad_one_is_an_error() {
        let dir = crate::testing::TempDir::new("baseline-read");
        let path = dir.path().join(FILE_NAME);
        assert!(Baseline::read(&path).unwrap().is_none());

        sample().write(&path).unwrap();
        assert_eq!(Baseline::read(&path).unwrap(), Some(sample()));

        std::fs::write(&path, "{\"version\": 1}").unwrap();
        let error = Baseline::read(&path).unwrap_err();
        assert!(matches!(error, BenchError::Parse { .. }), "{error:?}");
        assert!(
            error.to_string().ends_with(
                "baseline.json` is not a baseline this build reads: the file has no `machine`"
            ),
            "{error}"
        );

        std::fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
        let error = Baseline::read(&path).unwrap_err();
        assert!(matches!(error, BenchError::Read { .. }), "{error:?}");
        assert!(
            error.to_string().ends_with("it is over 1048576 bytes"),
            "{error}"
        );

        // A directory where the file should be cannot be read.
        let error = Baseline::read(dir.path()).unwrap_err();
        assert!(matches!(error, BenchError::Read { .. }), "{error:?}");
    }

    #[test]
    fn a_failed_save_says_whether_the_new_file_is_in_place() {
        let path = Path::new("bench/baseline.json");
        let failed = |step| {
            let error = TextError::Io {
                step,
                error: io::Error::other("injected"),
            };
            write_error(path, &error).to_string()
        };
        // Before the rename, the old file is untouched, and nothing says
        // otherwise.
        let early = failed(FileStep::Write);
        assert!(
            early.starts_with("cannot write `bench/baseline.json`: "),
            "{early}"
        );
        assert!(!early.contains("already holds"), "{early}");
        // After it, the message says the new baseline is there.
        assert_eq!(
            failed(FileStep::SyncDirectory),
            "cannot write `bench/baseline.json`: could not sync the directory (the file already \
             holds the new text): injected"
        );
        // A save into a directory that does not exist fails early.
        let dir = crate::testing::TempDir::new("baseline-write");
        let missing = dir.path().join("no-such-dir").join(FILE_NAME);
        let error = sample().write(&missing).unwrap_err();
        assert!(matches!(error, BenchError::Write { .. }), "{error:?}");
        assert!(!missing.exists());
    }

    #[test]
    fn a_merge_keeps_other_known_suites_and_drops_unknown_ones() {
        let mut existing = sample();
        existing.suites.insert("text/gone".to_string(), entry(999));
        let merged = Baseline::merge(
            Some(existing),
            machine("laptop"),
            [
                ("text/a".to_string(), entry(110)),
                ("text/c".to_string(), entry(300)),
            ],
            &["text/a", "text/b", "text/c"],
        )
        .unwrap();
        let names: Vec<(&str, u64)> = merged
            .suites
            .iter()
            .map(|(name, entry)| (name.as_str(), entry.median_ns))
            .collect();
        assert_eq!(names, [("text/a", 110), ("text/b", 200), ("text/c", 300)]);
        assert_eq!(merged.machine, machine("laptop"));

        // With no file, the merge is what was measured.
        let fresh =
            Baseline::merge(None, machine("new"), [("x".to_string(), entry(1))], &["x"]).unwrap();
        assert_eq!(fresh.suites.len(), 1);
        assert_eq!(fresh.machine.name, "new");
    }

    #[test]
    fn a_merge_into_another_machine_s_file_is_refused() {
        assert_eq!(
            Baseline::merge(Some(sample()), machine("desktop"), [], &[]).unwrap_err(),
            "it was recorded on `laptop`, not on `desktop`"
        );
        let mut other = machine("laptop");
        other.facts.threads = 8;
        other.facts.cpu = String::new();
        assert_eq!(
            Baseline::merge(Some(sample()), other, [], &[]).unwrap_err(),
            "it was recorded on `laptop` as linux x86_64, 16 threads, CPU \"11th Gen Intel(R) \
             Core(TM) i7-11800H @ 2.30GHz\", and this machine is linux x86_64, 8 threads, CPU \
             model unknown"
        );
    }

    #[test]
    fn the_cpu_model_is_the_first_model_name_line() {
        let cpuinfo = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel\t\t: 141\n\
                       model name\t: 11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz\n\
                       processor\t: 1\nmodel name\t: another\n";
        assert_eq!(
            cpu_model(cpuinfo),
            "11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz"
        );
        assert_eq!(cpu_model("processor : 0\nmodel : 1\n"), "");
        assert_eq!(cpu_model(""), "");
        assert_eq!(cpu_model("model name:\n"), "");
        // A file that is not there reads as no model.
        assert_eq!(read_cpu_model(Path::new("/nonexistent/cpuinfo")), "");
    }

    #[test]
    fn detected_facts_are_this_machine_s() {
        let facts = Facts::detect();
        assert_eq!(facts.os, std::env::consts::OS);
        assert_eq!(facts.arch, std::env::consts::ARCH);
        assert!(facts.threads >= 1);
        assert_eq!(facts, Facts::detect(), "the facts do not change");
    }

    #[test]
    fn the_default_path_is_in_the_crate_s_directory() {
        let path = default_path();
        assert!(path.ends_with("bench/baseline.json"), "{}", path.display());
        assert!(path.parent().unwrap().join("Cargo.toml").is_file());
    }
}
