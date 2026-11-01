//! SHA-256 (FIPS 180-4), for checking downloads against pinned digests
//! (ADR-0016).
//!
//! New in Tidepool; not ported. The round constants were derived from their
//! definition (the first 32 bits of the fractional parts of the cube roots
//! of the first 64 primes, and of the square roots of the first 8 primes for
//! the initial state) and match FIPS 180-4 §4.2.2 and §5.3.3.

use std::fmt;

/// The length of a SHA-256 digest in bytes.
pub const SHA256_LEN: usize = 32;

/// The block size in bytes.
const BLOCK: usize = 64;

/// Where the 64-bit message length goes in the last block.
const LENGTH_OFFSET: usize = BLOCK - 8;

/// FIPS 180-4 §4.2.2.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// FIPS 180-4 §5.3.3.
const INITIAL_STATE: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// A streaming SHA-256 computation.
///
/// Feed the data in any number of [`Sha256::update`] calls of any size, then
/// call [`Sha256::finalize`]. The digest is the same however the data was
/// split.
///
/// ```
/// use tidepool_core::hash::{Sha256, sha256, to_hex};
///
/// let mut hasher = Sha256::new();
/// hasher.update(b"a");
/// hasher.update(b"bc");
/// let digest = hasher.finalize();
/// assert_eq!(digest, sha256(b"abc"));
/// assert_eq!(
///     to_hex(&digest),
///     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
/// );
/// ```
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    /// Input not yet compressed. Only the first `filled` bytes are used.
    block: [u8; BLOCK],
    /// Always less than `BLOCK`.
    filled: usize,
    /// The message length in bits, modulo 2⁶⁴ as FIPS 180-4 §5.1.1 counts
    /// it. Wrapping, so no input length can make it panic.
    bit_length: u64,
}

impl Sha256 {
    /// Starts a new computation.
    pub const fn new() -> Self {
        Self {
            state: INITIAL_STATE,
            block: [0; BLOCK],
            filled: 0,
            bit_length: 0,
        }
    }

    /// Adds `data` to the message.
    pub fn update(&mut self, data: &[u8]) {
        // `usize` is at most 64 bits on every supported target, so the cast
        // is lossless; the multiply and add wrap as FIPS 180-4 allows.
        self.bit_length = self
            .bit_length
            .wrapping_add((data.len() as u64).wrapping_mul(8));

        let mut data = data;
        if self.filled > 0 {
            let take = (BLOCK - self.filled).min(data.len());
            let (head, rest) = data.split_at(take);
            self.block[self.filled..self.filled + take].copy_from_slice(head);
            self.filled += take;
            data = rest;
            if self.filled < BLOCK {
                return;
            }
            compress(&mut self.state, &self.block);
            self.filled = 0;
        }

        let (blocks, tail) = data.as_chunks::<BLOCK>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        self.block[..tail.len()].copy_from_slice(tail);
        self.filled = tail.len();
    }

    /// Pads the message (FIPS 180-4 §5.1.1) and returns its digest.
    pub fn finalize(mut self) -> [u8; SHA256_LEN] {
        // `filled < BLOCK`, so there is always room for the 0x80 byte.
        self.block[self.filled] = 0x80;
        self.filled += 1;
        if self.filled > LENGTH_OFFSET {
            self.block[self.filled..].fill(0);
            compress(&mut self.state, &self.block);
            self.filled = 0;
        }
        self.block[self.filled..LENGTH_OFFSET].fill(0);
        self.block[LENGTH_OFFSET..].copy_from_slice(&self.bit_length.to_be_bytes());
        compress(&mut self.state, &self.block);

        let mut digest = [0; SHA256_LEN];
        for (bytes, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *bytes = word.to_be_bytes();
        }
        digest
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

/// Shows only how much has been hashed, not the internal state.
impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sha256")
            .field("bit_length", &self.bit_length)
            .finish_non_exhaustive()
    }
}

/// Returns the SHA-256 digest of `data`.
pub fn sha256(data: &[u8]) -> [u8; SHA256_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

/// The compression function, FIPS 180-4 §6.2.2.
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK]) {
    let mut schedule = [0u32; 64];
    for (word, bytes) in schedule.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for t in 16..64 {
        let s0 = small_sigma0(schedule[t - 15]);
        let s1 = small_sigma1(schedule[t - 2]);
        schedule[t] = s1
            .wrapping_add(schedule[t - 7])
            .wrapping_add(s0)
            .wrapping_add(schedule[t - 16]);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (&k, &w) in K.iter().zip(&schedule) {
        let t1 = h
            .wrapping_add(big_sigma1(e))
            .wrapping_add(choose(e, f, g))
            .wrapping_add(k)
            .wrapping_add(w);
        let t2 = big_sigma0(a).wrapping_add(majority(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (word, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *word = word.wrapping_add(add);
    }
}

/// Ch, FIPS 180-4 (4.2).
const fn choose(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

/// Maj, FIPS 180-4 (4.3).
const fn majority(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

/// Σ₀, FIPS 180-4 (4.4).
const fn big_sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

/// Σ₁, FIPS 180-4 (4.5).
const fn big_sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

/// σ₀, FIPS 180-4 (4.6).
const fn small_sigma0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

/// σ₁, FIPS 180-4 (4.7).
const fn small_sigma1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::to_hex;
    use crate::testing::check_sized;

    fn hex(data: &[u8]) -> String {
        to_hex(&sha256(data))
    }

    // The digests below are the full values from the FIPS 180-4 examples
    // (NIST CSRC "Examples with Intermediate Values", SHA-256) and NIST's
    // additional SHA-256 vectors. Each was cross-checked in this port against
    // Python's `hashlib.sha256`.

    #[test]
    fn fips_vector_empty_message() {
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn fips_vector_abc() {
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn fips_vector_448_bit_two_block_message() {
        let message = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(message.len() * 8, 448);
        assert_eq!(
            hex(message),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn fips_vector_896_bit_message() {
        let message = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                        hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        assert_eq!(message.len() * 8, 896);
        assert_eq!(
            hex(message),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    #[test]
    fn fips_vector_one_million_a() {
        let expected = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
        assert_eq!(hex(&vec![b'a'; 1_000_000]), expected);

        // The same message streamed in odd-sized pieces.
        let mut hasher = Sha256::new();
        let piece = [b'a'; 997];
        let mut left = 1_000_000;
        while left > 0 {
            let take = left.min(piece.len());
            hasher.update(&piece[..take]);
            left -= take;
        }
        assert_eq!(to_hex(&hasher.finalize()), expected);
    }

    #[test]
    fn padding_boundaries() {
        // Lengths around the 55/56-byte and 64-byte padding edges, for the
        // pattern byte i = (7i + 3) mod 256. Expected digests are from
        // Python's `hashlib.sha256`.
        let cases = [
            (
                55,
                "e7313d333c272e639f790978283f9eb392e843d0f29b7016828bb1daa4aac70b",
            ),
            (
                56,
                "4324d65f3c103567f5589c710bc08f8523f929a9272e3af36fc968e52abc6c27",
            ),
            (
                57,
                "35df609437dcfea3279283ab79fd554e2bf78f8f7ae2de532d8ee300b09e8f73",
            ),
            (
                63,
                "81c80242132f230c3bd41b3e63bbcff16107339549214a99614ff26664625055",
            ),
            (
                64,
                "39e3d7b6b5d075d37d053ad89b24b41bef4f3c29760c84447cab3f3be1882241",
            ),
            (
                65,
                "aacca6ff74fdbb296d165a45cecfa04e5127bc008770fbbdd48006f2d2fae95e",
            ),
            (
                111,
                "67d9492e628fd376e0b2efec8ca2b99b123e202cf620deb270728df979b2f73e",
            ),
            (
                112,
                "96b928cff8528dbb99602c709a65b846cb6467acb8b722f0d758e4dc27bfc508",
            ),
            (
                119,
                "9ce7368e4daf32341631b492e80359dc9f594b48453cd0dd5bf0b19279cc177e",
            ),
            (
                120,
                "7836b787757e95e58b3ca5aec90b1b004e8deba1e50e9675af9cabf1a13a04b5",
            ),
            (
                127,
                "a8d23e75d936f303d248888d9b165ee543f4cbafcad3c9dd2a79bd84faa11d07",
            ),
            (
                128,
                "d2742f1f4ac6bb7ca2b239ee18402ba8b3f9f8e652d2a72973c2b9ba11c08cf6",
            ),
            (
                129,
                "307f8fc2c1622b92762e818d39a185d4d667ad49a4b07ceae1f4afa008a93ec4",
            ),
        ];
        for (len, expected) in cases {
            let data: Vec<u8> = (0..len).map(|i: u32| (i * 7 + 3) as u8).collect();
            assert_eq!(hex(&data), expected, "length {len}");
        }
    }

    #[test]
    fn streaming_leaves_no_stale_bytes_in_the_padding_block() {
        // 60 + 4 bytes fill the first block; the last 57 then leave the
        // first block's bytes 57..64 in the buffer, which padding must
        // clear before the extra length block. The digest of the same 121
        // pattern bytes, (7i + 3) mod 256, is from Python's
        // `hashlib.sha256`.
        let data: Vec<u8> = (0..121).map(|i: u32| (i * 7 + 3) as u8).collect();
        let expected = "1189a98a00c71bc1848ea8bdc9700b442bee0be7c3f45172303f1ab0b6f1617e";
        assert_eq!(hex(&data), expected);
        let mut hasher = Sha256::new();
        let (first, rest) = data.split_at(60);
        let (second, third) = rest.split_at(4);
        hasher.update(first);
        hasher.update(second);
        hasher.update(third);
        assert_eq!(to_hex(&hasher.finalize()), expected);
    }

    #[test]
    fn digest_length_and_default() {
        assert_eq!(sha256(b"x").len(), SHA256_LEN);
        assert_eq!(Sha256::default().finalize(), sha256(b""));
        let debug = format!("{:?}", Sha256::new());
        assert!(debug.starts_with("Sha256 { bit_length: 0"), "{debug}");
    }

    #[test]
    fn the_length_counter_wraps_instead_of_panicking() {
        // A message of 2⁶¹ bytes or more cannot be fed in a test, so set
        // the counter near its limit directly and check that updates wrap.
        let mut hasher = Sha256::new();
        hasher.bit_length = u64::MAX - 7;
        hasher.update(b"ab");
        assert_eq!(hasher.bit_length, 8);
        let _ = hasher.finalize();
    }

    #[test]
    fn random_chunking_matches_one_shot() {
        check_sized(
            "sha256 chunked equals one-shot",
            300,
            |rng, size| {
                // Up to 100 chunks of up to 150 bytes: crosses many block
                // boundaries in every alignment.
                (0..size)
                    .map(|_| {
                        let len = rng.range(0..150u32);
                        (0..len).map(|_| rng.next_u32() as u8).collect::<Vec<u8>>()
                    })
                    .collect::<Vec<_>>()
            },
            |chunks| {
                let whole: Vec<u8> = chunks.concat();
                let mut hasher = Sha256::new();
                for chunk in chunks {
                    hasher.update(chunk);
                }
                let streamed = hasher.finalize();
                let one_shot = sha256(&whole);
                if streamed != one_shot {
                    return Err(format!(
                        "streamed {} but one-shot {}",
                        to_hex(&streamed),
                        to_hex(&one_shot)
                    ));
                }
                Ok(())
            },
        );
    }
}
