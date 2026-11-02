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

//! Size limits that bound what a parser will accept.
//!
//! Ported from nadir-codec `limits.rs`. Adapted for Tidepool: the fields
//! that bound JSON (input size and number length are new), defaults sized
//! for `cargo metadata` and language-server traffic instead of game assets,
//! and a `const` [`Limits::DEFAULT`] like `tidepool_core::cli::Limits`. The
//! image and audio bounds stay in Nadir; decompression bounds arrive with
//! `inflate` and `zip`.

/// Upper bounds a parser enforces on its input, so that a malformed or
/// hostile document cannot exhaust memory or the stack.
///
/// Every bound is checked before the memory it guards is allocated.
/// Callers that need more (or less) build their own from the defaults, so
/// that fields added later for other formats keep their defaults:
///
/// ```
/// use tidepool_codec::Limits;
///
/// let limits = Limits {
///     max_input_bytes: 512 * 1024 * 1024,
///     ..Limits::DEFAULT
/// };
/// assert_eq!(limits.max_depth, Limits::DEFAULT.max_depth);
/// ```
///
/// A parsed document can take more memory than its text. For JSON, the
/// tree takes at most 16 bytes of heap per byte of text, plus a constant:
/// every value is 32 bytes and takes at least two bytes of text (`0,`), and
/// every array and object is allocated at exactly its size. Parsing needs a
/// little more while it runs, and the allocator adds its own overhead per
/// allocation: with glibc, the worst documents measured (many small
/// arrays, such as `[[0]],` repeated) grow the process by about 23 times
/// their text. Typical documents, made mostly of strings and objects, take
/// 2 to 4 times their text (`cargo metadata` output takes about 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The longest input accepted, in bytes. Checked first, before anything
    /// is parsed.
    pub max_input_bytes: usize,
    /// How deeply arrays and objects may nest. A document that is only a
    /// scalar has depth 0; `[[1]]` has depth 2.
    ///
    /// The JSON parser keeps its own stack on the heap, so parsing does not
    /// use more of the thread's stack as this grows, and when a parse fails
    /// it takes apart what it built without recursing. Dropping, cloning,
    /// comparing, formatting with `Debug` and writing the value a parse
    /// returns do recurse once per level, though, so keep this in the
    /// hundreds.
    pub max_depth: usize,
    /// The longest string, in bytes of UTF-8 after escapes are decoded.
    /// Applies to object keys as well.
    pub max_string_bytes: usize,
    /// The longest number, in bytes of its text (sign, digits, point and
    /// exponent).
    pub max_number_bytes: usize,
    /// The most elements one array, or members one object, may have. The
    /// bound is per container, not for the whole document.
    pub max_entries: usize,
}

impl Limits {
    /// The defaults:
    ///
    /// - `max_input_bytes`: 64 MiB. `cargo metadata` for a large workspace
    ///   with a deep dependency graph runs to tens of megabytes; this leaves
    ///   room above that while capping the parsed tree at 1 GiB of heap in
    ///   the worst case, about 1.5 GiB with glibc's overhead (see above).
    ///   Raise it for bigger inputs.
    /// - `max_depth`: 128. `cargo metadata` nests about 6 deep and language
    ///   server messages about 10; 128 is what most JSON libraries allow.
    /// - `max_string_bytes`: 16 MiB, enough for a language server's edit
    ///   that replaces a whole large file.
    /// - `max_number_bytes`: 1,024. The longest useful number is under 800
    ///   bytes (an `f64` is decided by its first 768 significant digits);
    ///   anything longer is padding.
    /// - `max_entries`: 1,000,000 per array or object, far above the
    ///   largest array in `cargo metadata` (one entry per package in the
    ///   dependency graph), and it bounds one array to 32 MB of values.
    pub const DEFAULT: Self = Self {
        max_input_bytes: 64 * 1024 * 1024,
        max_depth: 128,
        max_string_bytes: 16 * 1024 * 1024,
        max_number_bytes: 1024,
        max_entries: 1_000_000,
    };
}

impl Default for Limits {
    /// [`Limits::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_values() {
        let limits = Limits::default();
        assert_eq!(limits, Limits::DEFAULT);
        assert_eq!(limits.max_input_bytes, 67_108_864);
        assert_eq!(limits.max_depth, 128);
        assert_eq!(limits.max_string_bytes, 16_777_216);
        assert_eq!(limits.max_number_bytes, 1024);
        assert_eq!(limits.max_entries, 1_000_000);
    }
}
