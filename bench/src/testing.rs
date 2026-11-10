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

//! What the unit tests share: a temporary directory and a machine's facts.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::baseline::Facts;

/// A new, empty directory in the system's temporary directory, removed
/// when dropped.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// A directory whose name holds `name`, the process id and a counter,
    /// so tests running at once never share one.
    pub(crate) fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tidepool-bench-{name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("the temporary directory can be made");
        Self(path)
    }

    /// The directory.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a directory left behind is only clutter.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The reference machine's facts (`docs/00-overview.md`), whatever this
/// machine is.
pub(crate) fn facts() -> Facts {
    Facts {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        threads: 16,
        cpu: "11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz".to_string(),
    }
}
