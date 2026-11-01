//! Hashing: [`fnv1a64`] for stable keys, [`crc32`] for accidental-corruption
//! checks, [`FastHasher`] for deterministic in-memory hash maps, and
//! [`sha256`] for checking downloads against pinned digests.
//!
//! Ported from nadir-core `hash.rs` (`fnv1a64`, `crc32` and the Fx-style
//! hasher, renamed here to [`FastHasher`]). SHA-256 is new (ADR-0016).
//!
//! Only SHA-256 resists deliberate collisions, and Tidepool uses it for one
//! thing: comparing a downloaded file with a digest pinned in a tool
//! descriptor. There is no HMAC, signing or key handling here (ADR-0007,
//! ADR-0016). The other hashes must not be used where an attacker could
//! choose the input to cause collisions.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{BuildHasherDefault, Hasher};

mod sha256;

pub use self::sha256::{SHA256_LEN, Sha256, sha256};

/// Hashes `bytes` with 64-bit FNV-1a.
///
/// The result is stable across runs, platforms and Tidepool versions, so it
/// is safe to store on disk.
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

/// One table entry per byte value, for the reflected IEEE polynomial.
const CRC32_TABLE: [u32; 256] = {
    const POLYNOMIAL: u32 = 0xedb8_8320;

    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut crc = byte as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ POLYNOMIAL
            };
            bit += 1;
        }
        table[byte] = crc;
        byte += 1;
    }
    table
};

/// Computes the CRC-32 of `bytes`: the IEEE 802.3 checksum used by ZIP,
/// PNG, zlib and gzip.
///
/// It detects accidental corruption. It is not a defense against tampering.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let entry = CRC32_TABLE[usize::from(crc.to_le_bytes()[0] ^ byte)];
        crc = entry ^ (crc >> 8);
    }
    !crc
}

/// Formats `bytes` as lowercase hexadecimal, two digits per byte: the form
/// tool descriptors use for pinned SHA-256 digests.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// A fast, deterministic hasher for in-memory hash maps, in the style of Fx
/// hashing: rotate, xor, multiply, one word at a time.
///
/// It has no per-process random state, so a map that uses it iterates in an
/// order that depends only on the history of operations on that map.
///
/// **It is not HashDoS-resistant.** Someone who controls the keys can make
/// them all collide and turn every lookup into a linear scan. Use the
/// standard `HashMap` for keys that come from outside Tidepool in bulk
/// (network-sized inputs, untrusted files).
///
/// The output is not stable across Tidepool versions. Do not store it on
/// disk; use [`fnv1a64`] for that.
#[derive(Clone, Copy, Debug, Default)]
pub struct FastHasher {
    hash: u64,
}

impl FastHasher {
    /// An odd constant with well-spread bits: 2⁶⁴ / π.
    const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

    fn add_word(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(Self::MULTIPLIER);
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, tail) = bytes.as_chunks::<8>();
        for &word in words {
            self.add_word(u64::from_le_bytes(word));
        }
        if !tail.is_empty() {
            let mut last = [0; 8];
            last[..tail.len()].copy_from_slice(tail);
            self.add_word(u64::from_le_bytes(last));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add_word(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.add_word(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add_word(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add_word(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.add_word(value as u64);
    }

    fn finish(&self) -> u64 {
        // The multiply mixes upward, so the high bits are the best mixed.
        // Hash tables pick buckets from the low bits: rotate the good bits
        // down, or keys that differ only in their high bits would collide.
        self.hash.rotate_left(26)
    }
}

/// The [`BuildHasher`](std::hash::BuildHasher) behind [`FastHashMap`] and
/// [`FastHashSet`].
pub type FastBuildHasher = BuildHasherDefault<FastHasher>;

/// A [`HashMap`] that uses [`FastHasher`]. Create one with
/// `FastHashMap::default()`.
///
/// Iteration order depends only on the operations performed, never on a
/// per-process seed. Not HashDoS-resistant: see [`FastHasher`].
pub type FastHashMap<K, V> = HashMap<K, V, FastBuildHasher>;

/// A [`HashSet`] that uses [`FastHasher`]. Create one with
/// `FastHashSet::default()`. Not HashDoS-resistant: see [`FastHasher`].
pub type FastHashSet<T> = HashSet<T, FastBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check;
    use std::hash::{BuildHasher, Hash};

    fn fast(feed: impl FnOnce(&mut FastHasher)) -> u64 {
        let mut hasher = FastHasher::default();
        feed(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        // From the FNV reference test suite; re-checked with an independent
        // implementation.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn fnv1a64_works_in_const_context() {
        const KEY: u64 = fnv1a64(b"foobar");
        assert_eq!(KEY, 0x8594_4171_f739_67e8);
    }

    #[test]
    fn crc32_matches_reference_vectors() {
        // The standard CRC-32 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // The rest agree with zlib's `crc32`.
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xe8b7_be43);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
        let every_byte: Vec<u8> = (0..=u8::MAX).collect();
        assert_eq!(crc32(&every_byte), 0x2905_8c73);
    }

    #[test]
    fn crc32_detects_any_single_bit_flip() {
        check(
            "crc32 single bit flip",
            200,
            |rng| {
                let len = rng.range(1..65u32);
                let data: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
                (data, rng.range(0..len * 8) as usize)
            },
            |(data, bit)| {
                let mut flipped = data.clone();
                flipped[bit / 8] ^= 1 << (bit % 8);
                if crc32(&flipped) == crc32(data) {
                    return Err(format!("flipping the bit kept CRC {:#010x}", crc32(data)));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn to_hex_is_lowercase_and_two_digits_per_byte() {
        assert_eq!(to_hex(&[]), "");
        assert_eq!(to_hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
    }

    #[test]
    fn fast_hasher_output_is_pinned() {
        // Expected values were recomputed in this port with an independent
        // implementation of the algorithm. They pin the function across
        // platforms and processes; changing them changes every
        // FastHashMap's iteration order.
        assert_eq!(fast(|_| {}), 0);
        assert_eq!(fast(|h| h.write_u64(42)), 0x31ae_56f1_c979_df20);
        assert_eq!(fast(|h| h.write_u64(65536)), 0x882a_5400_0306_dc9c);
        assert_eq!(
            fast(|h| {
                h.write_u32(1);
                h.write_u32(2);
            }),
            0xffe6_3eaf_21a9_2f99
        );
        assert_eq!(fast(|h| h.write(b"hello")), 0x1a9b_7456_2122_95a9);
        assert_eq!(fast(|h| h.write(b"12345678")), 0xea0d_1082_17a1_c4b9);
        let twenty: Vec<u8> = (0..20).collect();
        assert_eq!(fast(|h| h.write(&twenty)), 0x68e7_75ec_0bbd_abcc);
    }

    #[test]
    fn integer_writes_agree_across_widths() {
        let expected = fast(|h| h.write_u64(200));
        assert_eq!(fast(|h| h.write_u8(200)), expected);
        assert_eq!(fast(|h| h.write_u16(200)), expected);
        assert_eq!(fast(|h| h.write_u32(200)), expected);
        assert_eq!(fast(|h| h.write_usize(200)), expected);
    }

    #[test]
    fn keys_that_differ_only_in_high_bits_spread_over_low_bits() {
        // Without the final rotate, every one of these would land in the
        // same bucket of a small table.
        let mut buckets = FastHashSet::default();
        for n in 0..256u64 {
            buckets.insert(fast(|h| h.write_u64(n << 32)) & 0xff);
        }
        assert!(
            buckets.len() > 128,
            "only {} of 256 buckets used",
            buckets.len()
        );
    }

    #[test]
    fn fast_hash_map_and_set_work() {
        let mut map = FastHashMap::default();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("a", 3);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a"), Some(&3));
        assert_eq!(map.get("c"), None);

        let mut set = FastHashSet::default();
        assert!(set.insert((1, 2)));
        assert!(!set.insert((1, 2)));
        assert!(set.contains(&(1, 2)));
    }

    #[test]
    fn hashing_is_the_same_for_every_hasher_instance() {
        fn hash_of(value: impl Hash) -> u64 {
            FastBuildHasher::default().hash_one(value)
        }
        assert_eq!(hash_of("buffer"), hash_of("buffer"));
        assert_eq!(hash_of((3u32, -7i32)), hash_of((3u32, -7i32)));
        assert_ne!(hash_of("buffer"), hash_of("window"));
    }

    #[test]
    fn iteration_order_depends_only_on_operation_history() {
        check(
            "fast hash map iteration order",
            50,
            // Each operation is (insert rather than remove, key).
            |rng| {
                (0..300)
                    .map(|_| (rng.range(0..4u32) != 0, rng.range(0..128u32)))
                    .collect::<Vec<_>>()
            },
            |ops| {
                let build = || {
                    let mut map = FastHashMap::default();
                    for &(insert, key) in ops {
                        if insert {
                            map.insert(key, key.wrapping_mul(3));
                        } else {
                            map.remove(&key);
                        }
                    }
                    map.into_iter().collect::<Vec<_>>()
                };
                let (first, second) = (build(), build());
                if first != second {
                    return Err(format!(
                        "same operations, different order:\n{first:?}\n{second:?}"
                    ));
                }
                Ok(())
            },
        );
    }
}
