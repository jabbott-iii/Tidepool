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

//! The suites, one module per crate measured. Each suite's name starts
//! with the crate's (`text/`), so that `cargo xtask bench text` runs all of
//! that crate's.

pub mod text;

use crate::Suite;

/// Every suite, in the order a run takes them.
pub fn all() -> Vec<Suite> {
    text::SUITES.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_lowercase_paths_under_their_crate() {
        let suites = all();
        let mut names: Vec<&str> = suites.iter().map(|suite| suite.name).collect();
        for name in &names {
            let parts: Vec<&str> = name.split('/').collect();
            assert!(parts.len() >= 3, "{name}: crate, area and case");
            assert!(parts.iter().all(|part| !part.is_empty()), "{name}");
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_/".contains(&b)),
                "{name}"
            );
        }
        assert!(names.iter().all(|name| name.starts_with("text/")));
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a name is used twice");
    }
}
