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

//! The `name` table ([`Font::name`](super::Font::name) says which records
//! are read).

use super::read::{Span, be16};
use super::{ErrorKind, NameId};
use crate::FontError;

/// Where the records and strings are (absolute offsets).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Names {
    records: usize,
    count: usize,
    strings: usize,
}

/// The size of a name record.
const RECORD: usize = 12;

/// US English, the language preferred.
const ENGLISH_US: u16 = 0x0409;

/// Whether a record of this platform and encoding is read: Windows' BMP
/// and full Unicode encodings, and the Unicode platform's.
fn readable(platform: u16, encoding: u16) -> bool {
    platform == 0 || (platform == 3 && matches!(encoding, 1 | 10))
}

/// How a record of this platform, encoding and language ranks, best first;
/// `None` for one that is not read.
fn rank(platform: u16, encoding: u16, language: u16) -> Option<u8> {
    match platform {
        3 if readable(platform, encoding) => Some(if language == ENGLISH_US { 0 } else { 1 }),
        0 => Some(2),
        _ => None,
    }
}

impl Names {
    /// Checks that every record lies inside the table, and the string of
    /// each that could be read.
    pub(crate) fn read(table: Span<'_>) -> Result<Self, FontError> {
        let format = table.u16(0, "the format")?;
        if format > 1 {
            return Err(table.error(
                0,
                ErrorKind::BadVersion {
                    found: u32::from(format),
                },
            ));
        }
        let count = usize::from(table.u16(2, "the number of records")?);
        let strings = usize::from(table.u16(4, "the strings' offset")?);
        table.bytes(6, count * RECORD, "the name records")?;
        for index in 0..count {
            let at = 6 + index * RECORD;
            let platform = table.u16(at, "a record's platform")?;
            let encoding = table.u16(at + 2, "a record's encoding")?;
            if !readable(platform, encoding) {
                continue;
            }
            let length = usize::from(table.u16(at + 8, "a string's length")?);
            let offset = usize::from(table.u16(at + 10, "a string's offset")?);
            // At most 3 × 65,535: no overflow.
            table.bytes(strings + offset, length, "a name's string")?;
        }
        Ok(Self {
            records: table.start() + 6,
            count,
            strings: table.start() + strings,
        })
    }

    /// The string with name ID `id`, from the best record that has it.
    pub(crate) fn get(&self, data: &[u8], id: NameId) -> Option<String> {
        let mut best: Option<(u8, usize)> = None;
        for index in 0..self.count {
            let at = self.records + index * RECORD;
            if be16(data, at + 6)? != id.0 {
                continue;
            }
            let rank = rank(be16(data, at)?, be16(data, at + 2)?, be16(data, at + 4)?);
            if let Some(rank) = rank
                && best.is_none_or(|(best, _)| rank < best)
            {
                best = Some((rank, at));
            }
        }
        let (_, at) = best?;
        let length = usize::from(be16(data, at + 8)?);
        let start = self.strings + usize::from(be16(data, at + 10)?);
        let bytes = data.get(start..start + length)?;
        Some(decode_utf16be(bytes))
    }
}

/// Decodes UTF-16BE lossily: an unpaired surrogate, or an odd byte at the
/// end, becomes U+FFFD.
fn decode_utf16be(bytes: &[u8]) -> String {
    let (pairs, rest) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|&pair| u16::from_be_bytes(pair));
    let mut text: String = char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if !rest.is_empty() {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_english_ranks_first_then_windows_then_unicode() {
        assert_eq!(rank(3, 1, ENGLISH_US), Some(0));
        assert_eq!(rank(3, 10, 0x0407), Some(1));
        assert_eq!(rank(0, 3, 0), Some(2));
        assert_eq!(rank(1, 0, 0), None);
        assert_eq!(rank(3, 0, ENGLISH_US), None);
        assert_eq!(rank(2, 1, 0), None);
    }

    #[test]
    fn utf16_decodes_lossily() {
        assert_eq!(decode_utf16be(b"\0S\0a"), "Sa");
        // A pair for U+1F600, and an unpaired high surrogate.
        assert_eq!(decode_utf16be(&[0xd8, 0x3d, 0xde, 0x00]), "\u{1f600}");
        assert_eq!(decode_utf16be(&[0xd8, 0x3d, 0, b'x']), "\u{fffd}x");
        assert_eq!(decode_utf16be(&[0, b'a', 0]), "a\u{fffd}");
        assert_eq!(decode_utf16be(&[]), "");
    }
}
