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

//! QOI, the "Quite OK Image" format (specification 1.0, qoiformat.org):
//! lossless images, which Tidepool's golden tests keep their reference
//! frames in (`cargo xtask golden`, `docs/04-quality.md`).
//!
//! Ported from nadir-codec `qoi.rs`. Adapted for Tidepool: the bound on a
//! side is [`Limits::max_image_dimension`] (Nadir's `max_texture_dimension`),
//! and the defaults are those of the largest framebuffer `tidepool-gfx`
//! makes, 16,384 pixels a side and 1 GiB of pixels, not those of a game's
//! textures; the hand-made vectors are public-API tests in `tests/qoi.rs`,
//! beside the property tests, and only the tests of private helpers stay
//! here.
//!
//! [`decode`] reads a file into an [`Image`], and [`encode`] writes one,
//! byte for byte as the reference encoder (`qoi.h`) does.
//!
//! ```
//! use tidepool_codec::Limits;
//! use tidepool_codec::qoi::{self, Channels, ColorSpace, Header};
//!
//! let header = Header {
//!     width: 2,
//!     height: 1,
//!     channels: Channels::Rgba,
//!     color_space: ColorSpace::Srgb,
//! };
//! let pixels = [255, 0, 0, 255, 255, 0, 0, 128];
//! let file = qoi::encode(&header, &pixels)?;
//! let image = qoi::decode(&file, Limits::DEFAULT)?;
//! assert_eq!(image.header, header);
//! assert_eq!(image.pixels, pixels);
//! # Ok::<(), tidepool_codec::CodecError>(())
//! ```
//!
//! Pixels are always RGBA8, 4 bytes each, rows from the top. The header's
//! channel count and color space describe the image to its reader; they do
//! not change how the pixels are stored. So a file with [`Channels::Rgb`]
//! still decodes to RGBA, with the alpha its chunks give: 255, unless one
//! of them sets another. The specification's colors are not premultiplied
//! by alpha, but nothing here depends on that: every byte given to
//! [`encode`] is what [`decode`] gives back.
//!
//! What the decoder rejects, although the reference decoder accepts it:
//!
//! - A run that goes past the last pixel.
//! - Chunks that run out before the last pixel, where the reference
//!   repeats the last pixel decoded. Here the end marker is then read as
//!   chunks, and the file is cut short or its end marker is wrong.
//! - An end marker that is missing or wrong, and anything after it.
//!
//! An encoder that follows the specification writes none of these. Like the
//! reference, the decoder also rejects a width or height of 0, and it
//! rejects images larger than its [`Limits`].

use std::fmt;

use crate::{CodecError, Limits};

/// What the pixels of an image hold, as its header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channels {
    /// Red, green and blue: the header says every alpha is 255. The
    /// encoder holds to it; the decoder gives whatever alpha the chunks do.
    /// Header value 3.
    Rgb,
    /// Red, green, blue and alpha. Header value 4.
    Rgba,
}

/// How the color channels are encoded, as the header says. Alpha is always
/// linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// sRGB-encoded color. Header value 0.
    Srgb,
    /// Linear color. Header value 1.
    Linear,
}

/// The description of an image, from the 14 bytes that start a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// The width in pixels, at least 1.
    pub width: u32,
    /// The height in pixels, at least 1.
    pub height: u32,
    /// What the pixels hold.
    pub channels: Channels,
    /// How the color channels are encoded.
    pub color_space: ColorSpace,
}

/// A decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Its header.
    pub header: Header,
    /// Its pixels: RGBA8, 4 bytes a pixel, rows from the top, `width × height
    /// × 4` bytes in all.
    pub pixels: Vec<u8>,
}

/// What is wrong with a QOI file. See [`CodecError::Qoi`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input does not start with `qoif`.
    NotQoi,
    /// The width or the height is 0.
    EmptyImage,
    /// The width or the height is more than
    /// [`Limits::max_image_dimension`].
    DimensionTooLarge {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// The pixels would take more than [`Limits::max_decoded_bytes`].
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The channel count is neither 3 nor 4.
    BadChannels {
        /// The value in the header.
        found: u8,
    },
    /// The color space is neither 0 nor 1.
    BadColorSpace {
        /// The value in the header.
        found: u8,
    },
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A run goes past the last pixel.
    RunPastEnd,
    /// The 8 bytes after the last pixel are not the end marker.
    BadEndMarker,
    /// Bytes follow the end marker.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotQoi => f.write_str("the file does not start with `qoif`"),
            Self::EmptyImage => f.write_str("the image is 0 pixels wide or tall"),
            Self::DimensionTooLarge { limit } => write!(
                f,
                "the image is wider or taller than the limit of {limit} pixels"
            ),
            Self::TooManyBytes { limit } => write!(
                f,
                "the pixels would take more than the limit of {limit} bytes"
            ),
            Self::BadChannels { found } => {
                write!(f, "the channel count is {found}, not 3 or 4")
            }
            Self::BadColorSpace { found } => {
                write!(f, "the color space is {found}, not 0 or 1")
            }
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::RunPastEnd => f.write_str("a run goes past the last pixel"),
            Self::BadEndMarker => f.write_str("the end marker is missing or wrong"),
            Self::TrailingBytes => f.write_str("there are bytes after the end marker"),
        }
    }
}

/// Why an image cannot be encoded. See [`CodecError::QoiEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeErrorKind {
    /// The width or the height is 0.
    EmptyImage,
    /// The pixels are not `width × height × 4` bytes.
    WrongDataSize {
        /// The width the header gives.
        width: u32,
        /// The height the header gives.
        height: u32,
        /// The number of bytes of pixels.
        found: usize,
    },
    /// A pixel of an image with [`Channels::Rgb`] has an alpha other than
    /// 255.
    AlphaInRgb {
        /// The pixel, counted from 0 in rows from the top.
        pixel: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => f.write_str("the image is 0 pixels wide or tall"),
            Self::WrongDataSize {
                width,
                height,
                found,
            } => write!(
                f,
                "the pixels are {found} bytes, not {width} × {height} × 4"
            ),
            Self::AlphaInRgb { pixel } => write!(
                f,
                "pixel {pixel} of an RGB image has an alpha other than 255"
            ),
        }
    }
}

const MAGIC: [u8; 4] = *b"qoif";
const HEADER_LEN: usize = 14;
const END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

// The tags. The two 8-bit ones are checked first: as 2-bit tags they would
// read as runs of 63 and 64, which therefore do not exist.
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;
const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const TAG_MASK: u8 = 0xc0;

/// What a chunk cut short lacks.
const REST: &str = "the rest of a chunk";

/// The most pixels one run chunk gives.
const MAX_RUN: u8 = 62;

/// Where a pixel is kept in the table of recently seen pixels.
fn slot([r, g, b, a]: [u8; 4]) -> usize {
    (usize::from(r) * 3 + usize::from(g) * 5 + usize::from(b) * 7 + usize::from(a) * 11) % 64
}

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Qoi { offset, kind }
}

/// Decodes a QOI file.
///
/// # Errors
///
/// [`CodecError::Qoi`], with the byte offset of the problem, for a file
/// that is malformed or breaks a limit of `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Image, CodecError> {
    let header = read_header(bytes, limits)?;
    // The header check keeps this within `max_decoded_bytes`.
    let len = pixel_bytes(&header)
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| error(4, too_many_bytes(limits)))?;
    let count = len / 4;
    // A chunk byte gives at most one run of 62 pixels, so a file too short
    // to hold that many is cut off, however it goes on. Checking this first
    // keeps a small file from claiming a large allocation.
    let room = bytes.len().saturating_sub(HEADER_LEN);
    if count.div_ceil(usize::from(MAX_RUN)) > room {
        return Err(cut_short(bytes, "more chunks"));
    }

    let mut pixels = Vec::with_capacity(len);
    let mut seen = [[0u8; 4]; 64];
    let mut px = [0, 0, 0, 255];
    let mut at = HEADER_LEN;
    while pixels.len() < len {
        let start = at;
        let [tag] = take(bytes, &mut at, "the next chunk")?;
        let mut run = 1;
        match tag {
            OP_RGB => {
                let [r, g, b] = take(bytes, &mut at, REST)?;
                px = [r, g, b, px[3]];
            }
            OP_RGBA => px = take(bytes, &mut at, REST)?,
            _ => match tag & TAG_MASK {
                OP_INDEX => px = seen[usize::from(tag & 0x3f)],
                OP_DIFF => {
                    let [r, g, b, a] = px;
                    px = [
                        r.wrapping_add((tag >> 4) & 3).wrapping_sub(2),
                        g.wrapping_add((tag >> 2) & 3).wrapping_sub(2),
                        b.wrapping_add(tag & 3).wrapping_sub(2),
                        a,
                    ];
                }
                OP_LUMA => {
                    let [second] = take(bytes, &mut at, REST)?;
                    let dg = (tag & 0x3f).wrapping_sub(32);
                    let [r, g, b, a] = px;
                    px = [
                        r.wrapping_add(dg).wrapping_sub(8).wrapping_add(second >> 4),
                        g.wrapping_add(dg),
                        b.wrapping_add(dg)
                            .wrapping_sub(8)
                            .wrapping_add(second & 0x0f),
                        a,
                    ];
                }
                _ => {
                    run = usize::from(tag & 0x3f) + 1;
                    if run > (len - pixels.len()) / 4 {
                        return Err(error(start, ErrorKind::RunPastEnd));
                    }
                }
            },
        }
        // The reference decoder stores the pixel after every chunk, runs
        // and lookups included, and encoders may count on it.
        seen[slot(px)] = px;
        for _ in 0..run {
            pixels.extend_from_slice(&px);
        }
    }

    let tail = bytes.get(at..).unwrap_or_default();
    match tail.split_first_chunk::<8>() {
        Some((marker, rest)) if *marker == END_MARKER => match rest {
            [] => Ok(Image { header, pixels }),
            _ => Err(error(at + END_MARKER.len(), ErrorKind::TrailingBytes)),
        },
        None if END_MARKER.starts_with(tail) => Err(cut_short(bytes, "the end marker")),
        _ => Err(error(at, ErrorKind::BadEndMarker)),
    }
}

/// Encodes an image, as the reference encoder would. `pixels` holds RGBA8,
/// 4 bytes a pixel, rows from the top.
///
/// # Errors
///
/// [`CodecError::QoiEncode`] for a width or height of 0, for pixels that
/// are not `width × height × 4` bytes, and for [`Channels::Rgb`] with an
/// alpha other than 255.
pub fn encode(header: &Header, pixels: &[u8]) -> Result<Vec<u8>, CodecError> {
    let Header {
        width,
        height,
        channels,
        color_space,
    } = *header;
    let refuse = |kind| Err(CodecError::QoiEncode { kind });
    if width == 0 || height == 0 {
        return refuse(EncodeErrorKind::EmptyImage);
    }
    if pixel_bytes(header) != u64::try_from(pixels.len()).ok() {
        return refuse(EncodeErrorKind::WrongDataSize {
            width,
            height,
            found: pixels.len(),
        });
    }
    let pixels = pixels.as_chunks::<4>().0;
    if channels == Channels::Rgb
        && let Some(pixel) = pixels.iter().position(|&[.., a]| a != 255)
    {
        return refuse(EncodeErrorKind::AlphaInRgb { pixel });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + pixels.len() + END_MARKER.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.push(match channels {
        Channels::Rgb => 3,
        Channels::Rgba => 4,
    });
    out.push(match color_space {
        ColorSpace::Srgb => 0,
        ColorSpace::Linear => 1,
    });

    let mut seen = [[0u8; 4]; 64];
    let mut previous = [0, 0, 0, 255];
    let mut run = 0u8;
    let last = pixels.len() - 1;
    for (index, &px) in pixels.iter().enumerate() {
        if px == previous {
            run += 1;
            if run == MAX_RUN || index == last {
                out.push(OP_RUN | (run - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run - 1));
            run = 0;
        }
        // Like the reference encoder, this stores a pixel only when it
        // writes one of the four chunks below, never for a run.
        let slot = slot(px);
        if seen[slot] == px {
            // `slot` is below 64.
            out.push(OP_INDEX | slot as u8);
        } else {
            seen[slot] = px;
            push_chunk(&mut out, previous, px);
        }
        previous = px;
    }
    out.extend_from_slice(&END_MARKER);
    Ok(out)
}

/// Writes the smallest chunk that turns `previous` into `px`, other than a
/// run or a lookup, in the reference encoder's order of preference.
fn push_chunk(out: &mut Vec<u8>, previous: [u8; 4], px: [u8; 4]) {
    let [r, g, b, a] = px;
    if a != previous[3] {
        out.extend_from_slice(&[OP_RGBA, r, g, b, a]);
        return;
    }
    // Differences wrap around: 1 - 2 is -1, and 255 + 1 is 0.
    let dr = r.wrapping_sub(previous[0]).cast_signed();
    let dg = g.wrapping_sub(previous[1]).cast_signed();
    let db = b.wrapping_sub(previous[2]).cast_signed();
    let small = |d: i8| (-2..=1).contains(&d);
    if small(dr) && small(dg) && small(db) {
        let bias = |d: i8| (d + 2).cast_unsigned();
        out.push(OP_DIFF | (bias(dr) << 4) | (bias(dg) << 2) | bias(db));
        return;
    }
    let dr_dg = dr.wrapping_sub(dg);
    let db_dg = db.wrapping_sub(dg);
    let near = |d: i8| (-8..=7).contains(&d);
    if (-32..=31).contains(&dg) && near(dr_dg) && near(db_dg) {
        let bias = |d: i8| (d + 8).cast_unsigned();
        out.extend_from_slice(&[
            OP_LUMA | (dg + 32).cast_unsigned(),
            (bias(dr_dg) << 4) | bias(db_dg),
        ]);
        return;
    }
    out.extend_from_slice(&[OP_RGB, r, g, b]);
}

/// Reads and checks the header.
fn read_header(bytes: &[u8], limits: Limits) -> Result<Header, CodecError> {
    if !MAGIC.starts_with(bytes.get(..MAGIC.len()).unwrap_or(bytes)) {
        return Err(error(0, ErrorKind::NotQoi));
    }
    let Some(
        &[
            _,
            _,
            _,
            _,
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
            channels,
            color_space,
        ],
    ) = bytes.first_chunk::<HEADER_LEN>()
    else {
        return Err(cut_short(bytes, "the rest of the header"));
    };
    let check_side = |side: u32, offset: usize| match side {
        0 => Err(error(offset, ErrorKind::EmptyImage)),
        side if side > limits.max_image_dimension => Err(error(
            offset,
            ErrorKind::DimensionTooLarge {
                limit: limits.max_image_dimension,
            },
        )),
        side => Ok(side),
    };
    let width = check_side(u32::from_be_bytes([w0, w1, w2, w3]), 4)?;
    let height = check_side(u32::from_be_bytes([h0, h1, h2, h3]), 8)?;
    let channels = match channels {
        3 => Channels::Rgb,
        4 => Channels::Rgba,
        found => return Err(error(12, ErrorKind::BadChannels { found })),
    };
    let color_space = match color_space {
        0 => ColorSpace::Srgb,
        1 => ColorSpace::Linear,
        found => return Err(error(13, ErrorKind::BadColorSpace { found })),
    };
    let header = Header {
        width,
        height,
        channels,
        color_space,
    };
    let fits = pixel_bytes(&header)
        .is_some_and(|len| u64::try_from(limits.max_decoded_bytes).is_ok_and(|limit| len <= limit));
    if !fits {
        return Err(error(4, too_many_bytes(limits)));
    }
    Ok(header)
}

fn too_many_bytes(limits: Limits) -> ErrorKind {
    ErrorKind::TooManyBytes {
        limit: limits.max_decoded_bytes,
    }
}

/// `width × height × 4`, unless that overflows a `u64`.
fn pixel_bytes(header: &Header) -> Option<u64> {
    u64::from(header.width)
        .checked_mul(u64::from(header.height))?
        .checked_mul(4)
}

/// Takes the next `N` bytes from `at`, which it moves past them, or fails
/// with what they should have been.
fn take<const N: usize>(
    bytes: &[u8],
    at: &mut usize,
    expected: &'static str,
) -> Result<[u8; N], CodecError> {
    let next = bytes
        .get(*at..)
        .and_then(<[u8]>::first_chunk::<N>)
        .ok_or_else(|| cut_short(bytes, expected))?;
    *at += N;
    Ok(*next)
}

/// The input ends where `expected` should be.
fn cut_short(bytes: &[u8], expected: &'static str) -> CodecError {
    error(bytes.len(), ErrorKind::UnexpectedEnd { expected })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_specified_hash() {
        assert_eq!(slot([0, 0, 0, 0]), 0);
        assert_eq!(slot([0, 0, 0, 255]), 53);
        assert_eq!(slot([100, 50, 200, 255]), 19);
        assert_eq!(slot([255, 255, 255, 255]), (255 * 26) % 64);
    }

    #[test]
    fn pixel_bytes_never_overflow() {
        let header = |width, height| Header {
            width,
            height,
            channels: Channels::Rgba,
            color_space: ColorSpace::Srgb,
        };
        assert_eq!(pixel_bytes(&header(3, 2)), Some(24));
        assert_eq!(pixel_bytes(&header(16_384, 16_384)), Some(1 << 30));
        // u32::MAX² fits a u64, but four times it does not.
        assert_eq!(pixel_bytes(&header(u32::MAX, u32::MAX)), None);
        assert_eq!(
            pixel_bytes(&header(u32::MAX, 1 << 30)),
            Some(u64::from(u32::MAX) << 32)
        );
    }

    #[test]
    fn the_end_marker_is_seven_zeros_and_a_one() {
        assert_eq!(END_MARKER, [0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(&MAGIC, b"qoif");
        assert_eq!(HEADER_LEN, 14);
    }

    #[test]
    fn error_kinds_explain_themselves() {
        let cases = [
            (ErrorKind::NotQoi, "the file does not start with `qoif`"),
            (ErrorKind::EmptyImage, "the image is 0 pixels wide or tall"),
            (
                ErrorKind::DimensionTooLarge { limit: 16_384 },
                "the image is wider or taller than the limit of 16384 pixels",
            ),
            (
                ErrorKind::TooManyBytes { limit: 16 },
                "the pixels would take more than the limit of 16 bytes",
            ),
            (
                ErrorKind::BadChannels { found: 5 },
                "the channel count is 5, not 3 or 4",
            ),
            (
                ErrorKind::BadColorSpace { found: 9 },
                "the color space is 9, not 0 or 1",
            ),
            (
                ErrorKind::UnexpectedEnd {
                    expected: "the end marker",
                },
                "the file ends where the end marker should be",
            ),
            (ErrorKind::RunPastEnd, "a run goes past the last pixel"),
            (
                ErrorKind::BadEndMarker,
                "the end marker is missing or wrong",
            ),
            (
                ErrorKind::TrailingBytes,
                "there are bytes after the end marker",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
        let cases = [
            (
                EncodeErrorKind::EmptyImage,
                "the image is 0 pixels wide or tall",
            ),
            (
                EncodeErrorKind::WrongDataSize {
                    width: 2,
                    height: 3,
                    found: 5,
                },
                "the pixels are 5 bytes, not 2 × 3 × 4",
            ),
            (
                EncodeErrorKind::AlphaInRgb { pixel: 7 },
                "pixel 7 of an RGB image has an alpha other than 255",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
    }
}
