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

//! QOI tests: hand-made vectors (the chunks the reference encoder writes
//! for small images, worked out by hand from the specification, and files
//! the decoder must reject, each at its byte), then property tests: random
//! images survive being encoded and decoded, and damaged files and random
//! chunks are decoded or rejected without a panic.
//!
//! Ported from nadir-codec `tests/qoi.rs` and the unit tests of its
//! `src/qoi.rs`. Adapted for Tidepool: the vectors are here, through the
//! public API, with the property tests; the default limits are 16,384
//! pixels a side and 1 GiB ([`Limits::DEFAULT`]); and there are vectors for
//! every kind of chunk in one image, for an RGB chunk keeping the alpha
//! before it, for limits met exactly, and for limits so high that only the
//! arithmetic stops a header, and a property test of random chunks.
//!
//! Every property test prints its seed and a command to rerun it if it
//! fails; set `TIDEPOOL_SEED` to run it with another seed.

use tidepool_codec::qoi::{self, Channels, ColorSpace, EncodeErrorKind, ErrorKind, Header, Image};
use tidepool_codec::{CodecError, Limits};
use tidepool_core::Pcg32;
use tidepool_core::testing::{check, check_sized};

const END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 255];

const fn header(width: u32, height: u32) -> Header {
    Header {
        width,
        height,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    }
}

/// A file: a header with these fields, `body`, and the end marker.
fn file(width: u32, height: u32, channels: u8, color_space: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"qoif".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[channels, color_space]);
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&END_MARKER);
    bytes
}

fn flat(pixels: &[[u8; 4]]) -> Vec<u8> {
    pixels.concat()
}

fn decode(bytes: &[u8]) -> Result<Image, CodecError> {
    qoi::decode(bytes, Limits::DEFAULT)
}

fn err(bytes: &[u8]) -> (usize, ErrorKind) {
    err_with(bytes, Limits::DEFAULT)
}

fn err_with(bytes: &[u8], limits: Limits) -> (usize, ErrorKind) {
    match qoi::decode(bytes, limits) {
        Err(CodecError::Qoi { offset, kind }) => (offset, kind),
        other => panic!("not a QOI error: {other:?}"),
    }
}

fn encode_err(header: &Header, pixels: &[u8]) -> EncodeErrorKind {
    match qoi::encode(header, pixels) {
        Err(CodecError::QoiEncode { kind }) => kind,
        other => panic!("not a QOI encoding error: {other:?}"),
    }
}

// ---------------------------------------------------------------- vectors

/// A name, the pixels of one row, and the chunks they encode to.
type Vector = (&'static str, Vec<[u8; 4]>, Vec<u8>);

/// Images of one row, and the chunks the reference encoder writes for
/// them, worked out by hand from the specification. The first pixel is
/// compared with opaque black, and the table of seen pixels starts as all
/// zeros.
fn vectors() -> Vec<Vector> {
    vec![
        // Equal to the starting pixel: a run of 1, 0b11_000000.
        ("starting pixel", vec![OPAQUE_BLACK], vec![0xc0]),
        // Transparent black is in slot 0 of the zeroed table:
        // QOI_OP_INDEX 0, which is the byte 0x00.
        ("zeroed table", vec![[0, 0, 0, 0]], vec![0x00]),
        // dr = 1, dg = -1 (0 - 1 wraps to 255), db = 0; biased by 2:
        // 0b01 11 01 10.
        ("diff", vec![[1, 255, 0, 255]], vec![0x76]),
        // dr = -1 (0 wraps to 255), dg = 1, db = 0: 0b01 01 11 10.
        ("diff wrapping", vec![[255, 1, 0, 255]], vec![0x5e]),
        // dg = 20, dr - dg = -8, db - dg = 7, the edges of the range:
        // 0b10 110100 (20 + 32), then 0000 1111 (-8 + 8, 7 + 8).
        ("luma", vec![[12, 20, 27, 255]], vec![0xb4, 0x0f]),
        // dg = -32 and 31, the edges of its range.
        ("luma low", vec![[224, 224, 224, 255]], vec![0x80, 0x88]),
        ("luma high", vec![[31, 31, 31, 255]], vec![0xbf, 0x88]),
        // dg = 32 is out of range, and so is dr - dg = 8.
        ("rgb", vec![[32, 32, 32, 255]], vec![0xfe, 32, 32, 32]),
        ("rgb dr", vec![[18, 10, 10, 255]], vec![0xfe, 18, 10, 10]),
        // A change of alpha always takes all four.
        ("rgba", vec![[1, 2, 3, 4]], vec![0xff, 1, 2, 3, 4]),
        // An RGB chunk keeps the alpha of the pixel before it: (100, 6, 7)
        // is too far from (1, 2, 3) for luma (dr - dg = 95).
        (
            "rgb keeps alpha",
            vec![[1, 2, 3, 4], [100, 6, 7, 4]],
            vec![0xff, 1, 2, 3, 4, 0xfe, 100, 6, 7],
        ),
        // (100, 50, 200, 255) is in slot 19 and opaque black in slot 53.
        // The third pixel is found in the table.
        (
            "index",
            vec![[100, 50, 200, 255], OPAQUE_BLACK, [100, 50, 200, 255]],
            vec![0xfe, 100, 50, 200, 0xfe, 0, 0, 0, 0x13],
        ),
        // A starting run, then transparent black from the zeroed table.
        (
            "run, then index",
            vec![OPAQUE_BLACK, [0, 0, 0, 0]],
            vec![0xc0, 0x00],
        ),
        // (5, 5, 5, 255): luma 0b10 100101 (5 + 32) and 1000 1000; then
        // 62 more, the longest run, 0b11 111101.
        (
            "longest run",
            [[5, 5, 5, 255]; 63].to_vec(),
            vec![0xa5, 0x88, 0xfd],
        ),
        // One more is a second run, of 1.
        (
            "run of 63",
            [[5, 5, 5, 255]; 64].to_vec(),
            vec![0xa5, 0x88, 0xfd, 0xc0],
        ),
        // A lookup is tried before a diff, and a diff before luma.
        (
            "lookup first",
            vec![[1, 1, 1, 255], [2, 2, 2, 255], [1, 1, 1, 255]],
            vec![0x7f, 0x7f, 0x04],
        ),
        // The encoder stores no pixel for a run, so opaque black is not in
        // slot 53 after the starting run: a diff, not a lookup (0x35).
        (
            "no store for runs",
            vec![OPAQUE_BLACK, OPAQUE_BLACK, [2, 2, 2, 255], OPAQUE_BLACK],
            vec![0xc1, 0xa2, 0x88, 0x40],
        ),
        // All three hash to slot 53. The second takes the slot.
        (
            "slot collision",
            vec![[64, 0, 0, 255], [0, 64, 0, 255], [64, 0, 0, 255]],
            vec![0xfe, 64, 0, 0, 0xfe, 0, 64, 0, 0xfe, 64, 0, 0],
        ),
        // dr = 10, dg = 8, db = 6, each after wrapping: luma
        // 0b10 101000 (8 + 32), then 1010 0110 (2 + 8, -2 + 8).
        (
            "luma wrapping",
            vec![[250, 250, 250, 255], [4, 2, 0, 255]],
            vec![0x9a, 0x88, 0xa8, 0xa6],
        ),
        // Translucent pixels with the same alpha take a diff, luma and RGB
        // chunk, as opaque ones do.
        (
            "translucent",
            vec![
                [10, 10, 10, 128],
                [11, 10, 10, 128],
                [21, 20, 20, 128],
                [100, 20, 20, 128],
            ],
            vec![0xff, 10, 10, 10, 128, 0x7a, 0xaa, 0x88, 0xfe, 100, 20, 20],
        ),
        // A run ends at a change of alpha, and at the last pixel.
        (
            "run and alpha",
            vec![
                [1, 1, 1, 255],
                [1, 1, 1, 255],
                [1, 1, 1, 255],
                [1, 1, 1, 0],
                [1, 1, 1, 0],
            ],
            vec![0x7f, 0xc1, 0xff, 1, 1, 1, 0, 0xc0],
        ),
        // A run ends before a pixel that differs.
        (
            "run, then diff",
            vec![
                [5, 5, 5, 255],
                [5, 5, 5, 255],
                [5, 5, 5, 255],
                [6, 5, 5, 255],
            ],
            vec![0xa5, 0x88, 0xc1, 0x7a],
        ),
    ]
}

#[test]
fn the_encoder_writes_what_the_reference_encoder_writes() {
    for (name, pixels, body) in vectors() {
        let width = pixels.len() as u32;
        let encoded = qoi::encode(&header(width, 1), &flat(&pixels)).unwrap();
        assert_eq!(encoded, file(width, 1, 4, 0, &body), "{name}");
    }
}

#[test]
fn the_decoder_reads_what_the_reference_encoder_writes() {
    for (name, pixels, body) in vectors() {
        let width = pixels.len() as u32;
        let image = decode(&file(width, 1, 4, 0, &body)).unwrap();
        assert_eq!(image.header, header(width, 1), "{name}");
        assert_eq!(image.pixels, flat(&pixels), "{name}");
    }
}

#[test]
fn every_kind_of_chunk_round_trips_in_one_image() {
    // A run (the starting pixel), a diff, luma, RGB, RGBA, and a lookup of
    // the pixel the diff stored in slot 4. The six pixels hash to slots
    // 53, 4, 11, 25, 36 and 4, so nothing else takes slot 4 first.
    let pixels = [
        OPAQUE_BLACK,
        [1, 1, 1, 255],
        [10, 10, 10, 255],
        [100, 10, 10, 255],
        [100, 10, 10, 128],
        [1, 1, 1, 255],
    ];
    let body = [
        0xc0, // run of 1
        0x7f, // diff +1 +1 +1
        0xa9, 0x88, // luma dg = 9, dr - dg = db - dg = 0
        0xfe, 100, 10, 10, // RGB
        0xff, 100, 10, 10, 128,  // RGBA
        0x04, // index 4
    ];
    let header = header(6, 1);
    let encoded = qoi::encode(&header, &flat(&pixels)).unwrap();
    assert_eq!(encoded, file(6, 1, 4, 0, &body));
    let image = decode(&encoded).unwrap();
    assert_eq!(image.header, header);
    assert_eq!(image.pixels, flat(&pixels));
    // Each tag is in the body: the 2-bit ones by their top bits.
    for (tag, mask) in [
        (0xc0, 0xc0),
        (0x40, 0xc0),
        (0x80, 0xc0),
        (0xfe, 0xff),
        (0xff, 0xff),
    ] {
        assert!(body.iter().any(|&byte| byte & mask == tag), "{tag:#x}");
    }
    assert!(body.contains(&0x04));
}

#[test]
fn the_decoder_stores_every_pixel_even_from_runs_and_lookups() {
    // The encoder writes 0xc1 for two opaque black pixels. Another encoder
    // may look the second up, in slot 53, which the run filled.
    let image = decode(&file(2, 1, 4, 0, &[0xc0, 0x35])).unwrap();
    assert_eq!(image.pixels, flat(&[OPAQUE_BLACK; 2]));
    let image = decode(&file(2, 1, 4, 0, &[0xc1])).unwrap();
    assert_eq!(image.pixels, flat(&[OPAQUE_BLACK; 2]));
    // A lookup stores what it found, again, in the slot it came from.
    let image = decode(&file(3, 1, 4, 0, &[0x00, 0x00, 0x00])).unwrap();
    assert_eq!(image.pixels, flat(&[[0, 0, 0, 0]; 3]));
}

#[test]
fn the_header_gives_the_channels_and_color_space() {
    let rgb_linear = Header {
        width: 2,
        height: 3,
        channels: Channels::Rgb,
        color_space: ColorSpace::Linear,
    };
    let pixels = flat(&[OPAQUE_BLACK; 6]);
    let encoded = qoi::encode(&rgb_linear, &pixels).unwrap();
    assert_eq!(encoded, file(2, 3, 3, 1, &[0xc5]));
    let image = decode(&encoded).unwrap();
    assert_eq!(image.header, rgb_linear);
    // RGB still decodes to RGBA.
    assert_eq!(image.pixels, pixels);
    // The channel count does not change how chunks are read: an RGB file
    // may hold an alpha.
    let image = decode(&file(1, 1, 3, 0, &[0xff, 1, 2, 3, 4])).unwrap();
    assert_eq!(image.pixels, [1, 2, 3, 4]);
    // And the other two combinations.
    for (channels, color_space, bytes) in [
        (Channels::Rgba, ColorSpace::Linear, [4, 1]),
        (Channels::Rgb, ColorSpace::Srgb, [3, 0]),
    ] {
        let header = Header {
            channels,
            color_space,
            ..header(1, 1)
        };
        let encoded = qoi::encode(&header, &OPAQUE_BLACK).unwrap();
        assert_eq!(encoded, file(1, 1, bytes[0], bytes[1], &[0xc0]));
        assert_eq!(decode(&encoded).unwrap().header, header);
    }
}

#[test]
fn rows_run_from_the_top() {
    let pixels = flat(&[
        [1, 0, 0, 255],
        [2, 0, 0, 255],
        [3, 0, 0, 255],
        [4, 0, 0, 255],
    ]);
    let image = decode(&qoi::encode(&header(2, 2), &pixels).unwrap()).unwrap();
    assert_eq!(image.pixels, pixels);
    // Runs cross rows.
    let encoded = qoi::encode(&header(3, 2), &flat(&[OPAQUE_BLACK; 6])).unwrap();
    assert_eq!(encoded, file(3, 2, 4, 0, &[0xc5]));
}

// ------------------------------------------------------------- rejections

#[test]
fn a_file_that_is_not_qoi_is_rejected() {
    assert_eq!(err(b"\x89PNG\r\n\x1a\n"), (0, ErrorKind::NotQoi));
    assert_eq!(err(b"qoiF"), (0, ErrorKind::NotQoi));
    assert_eq!(err(b"x"), (0, ErrorKind::NotQoi));
    // A start of the magic is a file cut short.
    let header = ErrorKind::UnexpectedEnd {
        expected: "the rest of the header",
    };
    assert_eq!(err(b""), (0, header.clone()));
    assert_eq!(err(b"qo"), (2, header.clone()));
    let whole = file(1, 1, 4, 0, &[0xc0]);
    assert_eq!(err(&whole[..13]), (13, header));
}

#[test]
fn header_fields_are_checked() {
    assert_eq!(err(&file(0, 1, 4, 0, &[])), (4, ErrorKind::EmptyImage));
    assert_eq!(err(&file(1, 0, 4, 0, &[])), (8, ErrorKind::EmptyImage));
    assert_eq!(
        err(&file(1, 1, 5, 0, &[0xc0])),
        (12, ErrorKind::BadChannels { found: 5 })
    );
    assert_eq!(
        err(&file(1, 1, 0, 0, &[0xc0])),
        (12, ErrorKind::BadChannels { found: 0 })
    );
    assert_eq!(
        err(&file(1, 1, 4, 2, &[0xc0])),
        (13, ErrorKind::BadColorSpace { found: 2 })
    );
}

#[test]
fn images_beyond_the_limits_are_rejected() {
    let too_wide = ErrorKind::DimensionTooLarge { limit: 16_384 };
    assert_eq!(err(&file(16_385, 1, 4, 0, &[])), (4, too_wide.clone()));
    assert_eq!(err(&file(1, 16_385, 4, 0, &[])), (8, too_wide.clone()));
    assert_eq!(err(&file(u32::MAX, u32::MAX, 4, 0, &[])), (4, too_wide));
    // The limits are configurable.
    let limits = Limits {
        max_image_dimension: 2,
        max_decoded_bytes: 12,
        ..Limits::DEFAULT
    };
    assert!(decode(&file(3, 1, 4, 0, &[0xc2])).is_ok());
    assert_eq!(
        err_with(&file(3, 1, 4, 0, &[0xc2]), limits),
        (4, ErrorKind::DimensionTooLarge { limit: 2 })
    );
    assert!(qoi::decode(&file(1, 2, 4, 0, &[0xc1]), limits).is_ok());
    assert_eq!(
        err_with(&file(2, 2, 4, 0, &[0xc3]), limits),
        (4, ErrorKind::TooManyBytes { limit: 12 })
    );
}

#[test]
fn an_image_exactly_at_the_limits_is_accepted() {
    let limits = Limits {
        max_image_dimension: 3,
        max_decoded_bytes: 12,
        ..Limits::DEFAULT
    };
    // 3 pixels a side, and 12 bytes: both limits met, neither passed.
    assert!(qoi::decode(&file(3, 1, 4, 0, &[0xc2]), limits).is_ok());
    assert!(qoi::decode(&file(1, 3, 4, 0, &[0xc2]), limits).is_ok());
    assert_eq!(
        err_with(&file(4, 1, 4, 0, &[0xc3]), limits),
        (4, ErrorKind::DimensionTooLarge { limit: 3 })
    );
    assert_eq!(
        err_with(
            &file(3, 1, 4, 0, &[0xc2]),
            Limits {
                max_decoded_bytes: 11,
                ..limits
            }
        ),
        (4, ErrorKind::TooManyBytes { limit: 11 })
    );
    // No image fits in no bytes.
    let none = Limits {
        max_decoded_bytes: 0,
        ..Limits::DEFAULT
    };
    assert_eq!(
        err_with(&file(1, 1, 4, 0, &[0xc0]), none),
        (4, ErrorKind::TooManyBytes { limit: 0 })
    );
    // The default dimensions fit the default bytes exactly.
    let largest = Limits::DEFAULT.max_image_dimension;
    assert_eq!(
        err(&file(largest, largest, 4, 0, &[0xfd; 4])).1,
        ErrorKind::UnexpectedEnd {
            expected: "more chunks"
        }
    );
}

#[test]
fn a_header_too_large_for_the_arithmetic_is_rejected_without_overflow() {
    // With no limit on the sides or the bytes, only the size of a u64
    // stops 4 × u32::MAX² bytes.
    let unbounded = Limits {
        max_image_dimension: u32::MAX,
        max_decoded_bytes: usize::MAX,
        ..Limits::DEFAULT
    };
    assert_eq!(
        err_with(&file(u32::MAX, u32::MAX, 4, 0, &[0xc0]), unbounded),
        (4, ErrorKind::TooManyBytes { limit: usize::MAX })
    );
    // 16 GiB of pixels pass the limits, and are refused for the file that
    // cannot hold them, before anything is allocated.
    let huge = file(u32::MAX, 1, 4, 0, &[0xfd; 100]);
    assert_eq!(
        err_with(&huge, unbounded),
        (
            huge.len(),
            ErrorKind::UnexpectedEnd {
                expected: "more chunks"
            }
        )
    );
}

#[test]
fn a_file_too_short_for_its_size_is_rejected_before_decoding() {
    // The largest image the default limits allow, in a file with no room
    // for its pixels: 256 Mi pixels need at least 4,329,605 chunk bytes.
    // This checks the error only; nothing here counts what is allocated.
    let more = ErrorKind::UnexpectedEnd {
        expected: "more chunks",
    };
    let huge = file(16_384, 16_384, 4, 0, &[0xfd; 1000]);
    assert_eq!(err(&huge), (huge.len(), more.clone()));
    // Shorter than a header and an end marker: nothing underflows.
    for len in [14, 21, 22] {
        assert_eq!(err(&huge[..len]), (len, more.clone()), "{len}");
    }
    // 64 pixels in two runs, the fewest bytes they fit in.
    assert!(decode(&file(8, 8, 4, 0, &[0xfd, 0xc1])).is_ok());
    // One run is all the room a 23-byte file has. A 63rd pixel is read
    // from the end marker, which then is wrong.
    assert!(decode(&file(62, 1, 4, 0, &[0xfd])).is_ok());
    assert_eq!(
        err(&file(63, 1, 4, 0, &[0xfd])),
        (16, ErrorKind::BadEndMarker)
    );
}

#[test]
fn a_file_cut_short_is_rejected() {
    // The end inside a chunk, or before the next one.
    let rest = ErrorKind::UnexpectedEnd {
        expected: "the rest of a chunk",
    };
    let mut bytes = file(1, 1, 4, 0, &[0xfe, 1]);
    bytes.truncate(16);
    assert_eq!(err(&bytes), (16, rest.clone()));
    let mut bytes = file(1, 1, 4, 0, &[0xff, 1, 2, 3]);
    bytes.truncate(18);
    assert_eq!(err(&bytes), (18, rest.clone()));
    let mut bytes = file(1, 1, 4, 0, &[0x80]);
    bytes.truncate(15);
    assert_eq!(err(&bytes), (15, rest));
    let mut bytes = file(2, 1, 4, 0, &[0x00]);
    bytes.truncate(15);
    let next = ErrorKind::UnexpectedEnd {
        expected: "the next chunk",
    };
    assert_eq!(err(&bytes), (15, next));
    // Every pixel there, and none or part of the end marker.
    let marker = ErrorKind::UnexpectedEnd {
        expected: "the end marker",
    };
    let whole = file(1, 1, 4, 0, &[0xc0]);
    for len in 15..whole.len() {
        assert_eq!(err(&whole[..len]), (len, marker.clone()), "{len}");
    }
}

#[test]
fn a_run_past_the_last_pixel_is_rejected() {
    assert_eq!(err(&file(1, 1, 4, 0, &[0xc1])), (14, ErrorKind::RunPastEnd));
    assert_eq!(
        err(&file(3, 1, 4, 0, &[0x00, 0xc2])),
        (15, ErrorKind::RunPastEnd)
    );
    assert!(decode(&file(3, 1, 4, 0, &[0x00, 0xc1])).is_ok());
}

#[test]
fn the_end_marker_must_be_there_and_be_last() {
    let mut bytes = file(1, 1, 4, 0, &[0xc0]);
    *bytes.last_mut().unwrap() = 2;
    assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
    // Every byte counts, not only the last.
    for at in [15, 18, 21] {
        let mut bytes = file(1, 1, 4, 0, &[0xc0]);
        bytes[at] = 0xff;
        assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker), "{at}");
    }
    // A wrong start, even if short.
    let bytes = [&file(1, 1, 4, 0, &[0xc0])[..15], &[0, 1]].concat();
    assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
    // A pixel too many is a wrong marker, as the image is complete.
    let bytes = file(1, 1, 4, 0, &[0xc0, 0xc0]);
    assert_eq!(err(&bytes), (15, ErrorKind::BadEndMarker));
    let mut bytes = file(1, 1, 4, 0, &[0xc0]);
    bytes.push(0);
    assert_eq!(err(&bytes), (23, ErrorKind::TrailingBytes));
    bytes.extend_from_slice(&END_MARKER);
    assert_eq!(err(&bytes), (23, ErrorKind::TrailingBytes));
}

#[test]
fn the_encoder_checks_its_input() {
    assert_eq!(encode_err(&header(0, 1), &[]), EncodeErrorKind::EmptyImage);
    assert_eq!(encode_err(&header(1, 0), &[]), EncodeErrorKind::EmptyImage);
    let wrong = |found| EncodeErrorKind::WrongDataSize {
        width: 1,
        height: 1,
        found,
    };
    assert_eq!(encode_err(&header(1, 1), &[0; 3]), wrong(3));
    assert_eq!(encode_err(&header(1, 1), &[0; 8]), wrong(8));
    // No overflow, however large the header.
    assert_eq!(
        encode_err(&header(u32::MAX, u32::MAX), &[0; 4]),
        EncodeErrorKind::WrongDataSize {
            width: u32::MAX,
            height: u32::MAX,
            found: 4
        }
    );
    let rgb = Header {
        channels: Channels::Rgb,
        ..header(3, 1)
    };
    assert_eq!(
        encode_err(&rgb, &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 254]),
        EncodeErrorKind::AlphaInRgb { pixel: 2 }
    );
}

#[test]
fn errors_say_qoi_and_where() {
    let error = decode(&file(1, 1, 4, 2, &[0xc0])).unwrap_err();
    assert_eq!(
        error.to_string(),
        "QOI error at byte 13: the color space is 2, not 0 or 1"
    );
    let error = qoi::encode(&header(0, 1), &[]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot encode as QOI: the image is 0 pixels wide or tall"
    );
}

// ------------------------------------------------------------- properties

/// A random image of about `size` pixels a row, made of the patterns each
/// chunk is for: repeats (runs), small and medium changes (diffs and luma),
/// colors seen before (lookups), and noise.
fn random_image(rng: &mut Pcg32, size: u32) -> Image {
    let width = 1 + rng.range(0..size + 1);
    let height = 1 + rng.range(0..size / 4 + 1);
    let channels = if rng.range(0..2u32) == 0 {
        Channels::Rgb
    } else {
        Channels::Rgba
    };
    let color_space = if rng.range(0..2u32) == 0 {
        ColorSpace::Srgb
    } else {
        ColorSpace::Linear
    };
    let palette: Vec<[u8; 4]> = (0..1 + rng.range(0..8u32))
        .map(|_| byte4(rng, channels))
        .collect();
    let mut px = OPAQUE_BLACK;
    let mut pixels = Vec::new();
    for _ in 0..width * height {
        px = match rng.range(0..6u32) {
            0 | 1 => px,
            2 => nudge(rng, px, 2),
            3 => nudge(rng, px, 40),
            4 => *rng.choose(&palette).unwrap_or(&px),
            _ => byte4(rng, channels),
        };
        if channels == Channels::Rgba && rng.range(0..16u32) == 0 {
            px[3] = rng.range(0..256u32) as u8;
        }
        pixels.extend_from_slice(&px);
    }
    Image {
        header: Header {
            width,
            height,
            channels,
            color_space,
        },
        pixels,
    }
}

fn byte4(rng: &mut Pcg32, channels: Channels) -> [u8; 4] {
    let mut byte = || rng.range(0..256u32) as u8;
    let alpha = match channels {
        Channels::Rgb => 255,
        Channels::Rgba => byte(),
    };
    [byte(), byte(), byte(), alpha]
}

/// Moves each color channel by up to `by` either way, wrapping.
fn nudge(rng: &mut Pcg32, [r, g, b, a]: [u8; 4], by: i32) -> [u8; 4] {
    let mut step = |channel: u8| channel.wrapping_add_signed(rng.range(-by..by + 1) as i8);
    [step(r), step(g), step(b), a]
}

#[test]
fn random_images_survive_being_encoded_and_decoded() {
    check_sized("qoi round-trip", 400, random_image, |image| {
        let encoded = qoi::encode(&image.header, &image.pixels)
            .map_err(|error| format!("cannot encode: {error}"))?;
        let decoded = decode(&encoded).map_err(|error| format!("cannot decode: {error}"))?;
        if decoded != *image {
            return Err(format!("decodes as {decoded:?}"));
        }
        // Encoding is deterministic, and what it wrote is what it reads.
        let again = qoi::encode(&decoded.header, &decoded.pixels)
            .map_err(|error| format!("cannot encode again: {error}"))?;
        if again != encoded {
            return Err("a second encoding differs".to_string());
        }
        Ok(())
    });
}

#[test]
fn every_cut_short_file_is_rejected() {
    check_sized(
        "qoi prefixes",
        100,
        |rng, size| {
            let image = random_image(rng, size);
            qoi::encode(&image.header, &image.pixels).expect("a random image encodes")
        },
        |encoded| {
            // Each prefix fails where it ends, even a prefix of the magic.
            for len in 0..encoded.len() {
                match decode(&encoded[..len]) {
                    Err(CodecError::Qoi {
                        offset,
                        kind: ErrorKind::UnexpectedEnd { .. },
                    }) if offset == len => {}
                    other => return Err(format!("{len} bytes give {other:?}")),
                }
            }
            Ok(())
        },
    );
}

/// What a decode of `bytes` must be, whatever they are: an image of the
/// size its header says, or a QOI error inside the file (at its end, for a
/// file cut short).
fn decoded_or_rejected(bytes: &[u8], result: &Result<Image, CodecError>) -> Result<(), String> {
    match result {
        Ok(image) => {
            let expected = image.header.width as usize * image.header.height as usize * 4;
            if image.pixels.len() == expected {
                Ok(())
            } else {
                Err(format!(
                    "{} bytes of pixels, not {expected}",
                    image.pixels.len()
                ))
            }
        }
        Err(CodecError::Qoi {
            offset,
            kind: ErrorKind::UnexpectedEnd { .. },
        }) if *offset != bytes.len() => Err(format!(
            "cut short at {offset}, but the file is {} bytes",
            bytes.len()
        )),
        Err(CodecError::Qoi { offset, .. }) if *offset <= bytes.len() => Ok(()),
        Err(error) => Err(format!("{error} points outside the file")),
    }
}

#[test]
fn damaged_files_are_decoded_or_rejected_without_a_panic() {
    check(
        "qoi damage",
        2000,
        |rng| {
            let image = random_image(rng, 16);
            let mut bytes =
                qoi::encode(&image.header, &image.pixels).expect("a random image encodes");
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = rng.range(0..256u32) as u8;
            }
            bytes
        },
        |bytes| decoded_or_rejected(bytes, &decode(bytes)),
    );
}

/// A file of a small random size and random chunks, each tag as likely as
/// the others, then most often the end marker: what another encoder, or
/// none, might write.
fn random_chunks(rng: &mut Pcg32, size: u32) -> Vec<u8> {
    let width = 1 + rng.range(0..size / 4 + 1);
    let height = 1 + rng.range(0..size / 8 + 1);
    let mut body = Vec::new();
    for _ in 0..rng.range(0..size + 1) {
        let mut byte = || rng.range(0..256u32) as u8;
        match byte() % 6 {
            0 => body.push(byte() & 0x3f),
            1 => body.push(0x40 | (byte() & 0x3f)),
            2 => body.extend_from_slice(&[0x80 | (byte() & 0x3f), byte()]),
            3 => body.push(0xc0 | (byte() % 62)),
            4 => body.extend_from_slice(&[0xfe, byte(), byte(), byte()]),
            _ => body.extend_from_slice(&[0xff, byte(), byte(), byte(), byte()]),
        }
    }
    let channels = 3 + (rng.range(0..2u32) as u8);
    let color_space = rng.range(0..2u32) as u8;
    let mut bytes = file(width, height, channels, color_space, &body);
    if rng.range(0..4u32) == 0 {
        bytes.truncate(bytes.len() - END_MARKER.len());
    }
    bytes
}

#[test]
fn random_chunks_decode_to_an_image_that_round_trips_or_are_rejected() {
    check_sized("qoi random chunks", 2000, random_chunks, |bytes| {
        let result = decode(bytes);
        decoded_or_rejected(bytes, &result)?;
        let Ok(image) = result else { return Ok(()) };
        // Chunks another encoder wrote need not be the reference encoder's,
        // but they decode to an image that it writes and reads back, unless
        // an RGB file's chunks gave an alpha other than 255.
        let first_alpha = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .position(|&[.., alpha]| alpha != 255);
        match (
            image.header.channels,
            first_alpha,
            qoi::encode(&image.header, &image.pixels),
        ) {
            (Channels::Rgb, Some(pixel), Err(CodecError::QoiEncode { kind })) => {
                if kind == (EncodeErrorKind::AlphaInRgb { pixel }) {
                    Ok(())
                } else {
                    Err(format!("refused for {kind:?}"))
                }
            }
            (Channels::Rgb, Some(_), Ok(_)) => Err("an RGB image with alpha encodes".into()),
            (_, _, Ok(encoded)) => match decode(&encoded) {
                Ok(again) if again == image => Ok(()),
                other => Err(format!("re-encoded, it decodes as {other:?}")),
            },
            (_, _, Err(error)) => Err(format!("cannot encode what decoded: {error}")),
        }
    });
}
