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

//! The suites of `tidepool-workspace`: opening a 100 MB file, against the
//! budget of `docs/00-overview.md` (at most 1 s, in large-file mode above
//! 16 MB; M4.11, ADR-0068).
//!
//! - `workspace/buffers/open/100mb`: a file of at least 100 MB of short
//!   lines (`line 00000: the quick brown fox é 😀`, numbered from 0 to
//!   99,999 and again, about 40 bytes each, every seventh ending in CR LF,
//!   as the text suites build theirs), opened as the main loop and a
//!   worker open one: a [`Buffers::open`] job run, then
//!   [`Buffers::finish_load`]. That canonicalizes the path, reads and
//!   decodes the file in 64 KiB blocks, counts its line endings, and makes
//!   the buffer, read-only as a large file.
//!
//! The file is written once, before the timing, in a folder the suite
//! makes anew in the system's temporary folder, under a name no other run
//! has, and removes when it ends. Each
//! iteration opens it into a new [`Buffers`], dropped outside the timing.
//! The file is then in the page cache, as a file just written or read
//! again is; a first open from a cold disk is slower by the disk's read
//! time, which no run here measures. Before it is measured, the suite
//! checks once that the buffer holds the whole file, read-only.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tidepool_workspace::buffers::{Buffers, ReadOnly};

use crate::{BenchError, Bencher, Suite};

/// The workspace suites, in the order a run takes them.
pub const SUITES: &[Suite] = &[Suite {
    name: "workspace/buffers/open/100mb",
    run: open_100mb,
}];

/// The size the file reaches or passes, in bytes.
const BIG_BYTES: usize = 100_000_000;

/// A scratch folder in the system's temporary folder, removed when
/// dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a leftover folder only takes space.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn failed(what: &str, error: impl std::fmt::Display) -> BenchError {
    BenchError::Check(format!("{what}: {error}"))
}

/// The text of the file: short numbered lines until [`BIG_BYTES`].
fn big_text() -> String {
    let mut text = String::with_capacity(BIG_BYTES + 64);
    let mut i = 0usize;
    while text.len() < BIG_BYTES {
        // Writing to a `String` cannot fail.
        let _ = write!(text, "line {:05}: the quick brown fox é 😀", i % 100_000);
        text.push_str(if i % 7 == 3 { "\r\n" } else { "\n" });
        i += 1;
    }
    text
}

fn open_100mb(b: &mut Bencher) -> Result<(), BenchError> {
    // A name no other run has, made anew: a folder that exists already,
    // perhaps another user's holding a link, fails the suite rather than
    // being written into.
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "tidepool-bench-open-{}-{since}",
        std::process::id()
    ));
    fs::create_dir(&dir).map_err(|error| failed("make a fresh scratch folder", error))?;
    let scratch = Scratch(dir);
    let path = scratch.0.join("big.txt");
    let text = big_text();
    fs::write(&path, &text).map_err(|error| failed("write the 100 MB file", error))?;
    let len = text.len();
    drop(text);

    // Checked once, untimed: the whole file, read-only as a large file.
    let mut buffers = Buffers::new();
    let opened = buffers
        .finish_load(buffers.open(&path).run())
        .map_err(|error| failed("open the file", error))?;
    let buffer = buffers
        .get(opened.id)
        .ok_or_else(|| BenchError::Check("the buffer opened is not there".to_string()))?;
    if buffer.text().len() != len || buffer.read_only() != Some(ReadOnly::Large) {
        return Err(BenchError::Check(format!(
            "the buffer holds {} of {len} bytes, read-only {:?}",
            buffer.text().len(),
            buffer.read_only()
        )));
    }
    drop(buffers);

    b.iter_batched(
        || {
            let buffers = Buffers::new();
            let job = buffers.open(&path);
            (buffers, job)
        },
        |(mut buffers, job)| {
            let opened = buffers.finish_load(job.run());
            (buffers, opened.is_ok())
        },
    );
    drop(scratch);
    Ok(())
}
