//! The keyed hash behind the node arena's structural fingerprint.
//!
//! SipHash-1-3 under a per-process random 128-bit key, as the standard
//! library's `RandomState` hashes, over a word encoding of the hashed
//! value: every `Hasher` write is laid out as whole little-endian 64-bit
//! words (an integer as one word; a byte slice as its length word, then its
//! bytes zero-padded to their last word), and the message is the
//! concatenation of those words. The length word keeps a slice's trailing
//! zero bytes distinct from its padding, so the encoding of a value's writes
//! is decodable: equal values write equal word streams, distinct values of
//! one type write distinct streams, and a writer that cannot read the key
//! cannot steer two distinct values onto one fingerprint.
//!
//! The initialization constants, `SipRound` and finalization follow
//! J.-P. Aumasson and D. J. Bernstein, "SipHash: a fast short-input PRF"
//! (INDOCRYPT 2012, <https://www.aumasson.jp/siphash/siphash.pdf>, §2);
//! SipHash-c-d runs `c` rounds per message word and `d` finalization
//! rounds, and the standard library's hasher is the 1-3 variant.
//!
//! The standard hasher folds each write into a byte buffer before it
//! compresses whole words; a payload's `Hash` impl writes one field at a
//! time, mostly integers, so this hasher compresses each integer write as
//! its own word and never assembles bytes into one.

use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;

/// The process's fingerprint key: two words drawn from a fresh
/// `RandomState`, so they are as unpredictable as its own key.
fn fingerprint_key() -> (u64, u64) {
    static KEY: OnceLock<(u64, u64)> = OnceLock::new();
    *KEY.get_or_init(|| {
        let seed = std::collections::hash_map::RandomState::new();
        (seed.hash_one(0u64), seed.hash_one(1u64))
    })
}

/// A SipHash-1-3 state keyed by this process's fingerprint key.
pub(super) struct FingerprintHasher {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    /// Message bytes compressed so far.
    length: u64,
}

impl FingerprintHasher {
    pub(super) fn new() -> Self {
        let (k0, k1) = fingerprint_key();
        Self::with_keys(k0, k1)
    }

    fn with_keys(k0: u64, k1: u64) -> Self {
        Self {
            v0: k0 ^ 0x736f_6d65_7073_6575,
            v1: k1 ^ 0x646f_7261_6e64_6f6d,
            v2: k0 ^ 0x6c79_6765_6e65_7261,
            v3: k1 ^ 0x7465_6462_7974_6573,
            length: 0,
        }
    }

    /// One `SipRound`. Every interned payload's fingerprint runs it per
    /// word, so its rotations are written as shifts, which a debug build
    /// emits in place rather than as a call per rotation.
    #[inline]
    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = (self.v1 << 13) | (self.v1 >> 51);
        self.v1 ^= self.v0;
        self.v0 = (self.v0 << 32) | (self.v0 >> 32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = (self.v3 << 16) | (self.v3 >> 48);
        self.v3 ^= self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = (self.v3 << 21) | (self.v3 >> 43);
        self.v3 ^= self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = (self.v1 << 17) | (self.v1 >> 47);
        self.v1 ^= self.v2;
        self.v2 = (self.v2 << 32) | (self.v2 >> 32);
    }

    /// Compress one message word: one compression round.
    #[inline]
    fn word(&mut self, word: u64) {
        self.v3 ^= word;
        self.round();
        self.v0 ^= word;
        self.length = self.length.wrapping_add(8);
    }
}

impl Hasher for FingerprintHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.word(bytes.len() as u64);
        let mut at = 0;
        while at < bytes.len() {
            let mut word = 0u64;
            let mut shift = 0;
            while shift < 64 && at < bytes.len() {
                word |= u64::from(bytes[at]) << shift;
                shift += 8;
                at += 1;
            }
            self.word(word);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.word(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.word(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.word(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.word(value);
    }

    fn write_u128(&mut self, value: u128) {
        self.word(value as u64);
        self.word((value >> 64) as u64);
    }

    fn write_usize(&mut self, value: usize) {
        self.word(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.write_u8(value as u8);
    }

    fn write_i16(&mut self, value: i16) {
        self.write_u16(value as u16);
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u32(value as u32);
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_i128(&mut self, value: i128) {
        self.write_u128(value as u128);
    }

    fn write_isize(&mut self, value: isize) {
        self.write_usize(value as usize);
    }

    /// The final block carries the message length's low byte, then three
    /// finalization rounds, as SipHash-1-3 ends.
    fn finish(&self) -> u64 {
        let mut state = Self {
            v0: self.v0,
            v1: self.v1,
            v2: self.v2,
            v3: self.v3,
            length: self.length,
        };
        let last = (state.length & 0xff) << 56;
        state.v3 ^= last;
        state.round();
        state.v0 ^= last;
        state.v2 ^= 0xff;
        state.round();
        state.round();
        state.round();
        state.v0 ^ state.v1 ^ state.v2 ^ state.v3
    }
}

#[cfg(test)]
mod tests {
    use super::FingerprintHasher;
    use std::hash::Hasher;

    /// The word stream a write lays out, as the bytes SipHash reads.
    fn message(words: &[u64]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    /// SipHash-c-d over a byte message, from the reference definition (the
    /// module's cited paper, §2), so the word-at-a-time state is checked
    /// against an independent byte-oriented one. Its constants and rounds
    /// are themselves checked against the paper's published vector and the
    /// standard library's SipHash-2-4.
    fn reference(compression: usize, finalization: usize, k0: u64, k1: u64, message: &[u8]) -> u64 {
        let mut v = [
            k0 ^ 0x736f_6d65_7073_6575,
            k1 ^ 0x646f_7261_6e64_6f6d,
            k0 ^ 0x6c79_6765_6e65_7261,
            k1 ^ 0x7465_6462_7974_6573,
        ];
        let round = |v: &mut [u64; 4]| {
            v[0] = v[0].wrapping_add(v[1]);
            v[1] = v[1].rotate_left(13) ^ v[0];
            v[0] = v[0].rotate_left(32);
            v[2] = v[2].wrapping_add(v[3]);
            v[3] = v[3].rotate_left(16) ^ v[2];
            v[0] = v[0].wrapping_add(v[3]);
            v[3] = v[3].rotate_left(21) ^ v[0];
            v[2] = v[2].wrapping_add(v[1]);
            v[1] = v[1].rotate_left(17) ^ v[2];
            v[2] = v[2].rotate_left(32);
        };
        let mut blocks = message.chunks_exact(8);
        for block in &mut blocks {
            let m = u64::from_le_bytes(block.try_into().unwrap());
            v[3] ^= m;
            for _ in 0..compression {
                round(&mut v);
            }
            v[0] ^= m;
        }
        let mut last = (message.len() as u64 & 0xff) << 56;
        for (index, byte) in blocks.remainder().iter().enumerate() {
            last |= u64::from(*byte) << (8 * index);
        }
        v[3] ^= last;
        for _ in 0..compression {
            round(&mut v);
        }
        v[0] ^= last;
        v[2] ^= 0xff;
        for _ in 0..finalization {
            round(&mut v);
        }
        v[0] ^ v[1] ^ v[2] ^ v[3]
    }

    /// SipHash-1-3 as the arena fingerprints with it.
    fn siphash_1_3(k0: u64, k1: u64, message: &[u8]) -> u64 {
        reference(1, 3, k0, k1, message)
    }

    #[test]
    #[allow(deprecated)] // `SipHasher` is std's stable SipHash-2-4.
    fn the_reference_definition_matches_published_siphash_2_4() {
        let (k0, k1) = (0x0706_0504_0302_0100, 0x0f0e_0d0c_0b0a_0908);
        let message: Vec<u8> = (0..15).collect();
        // The paper's Appendix A vector: key 00..0f, message 00..0e.
        assert_eq!(reference(2, 4, k0, k1, &message), 0xa129_ca61_49be_45e5);
        for length in 0..=24 {
            let message: Vec<u8> = (0..length).map(|byte| byte * 7 + 1).collect();
            let mut std_hasher = std::hash::SipHasher::new_with_keys(k0, k1);
            std_hasher.write(&message);
            assert_eq!(
                reference(2, 4, k0, k1, &message),
                std_hasher.finish(),
                "length {length}"
            );
        }
    }

    #[test]
    fn word_writes_hash_as_siphash_1_3_of_their_word_stream() {
        let (k0, k1) = (0x0706_0504_0302_0100, 0x0f0e_0d0c_0b0a_0908);
        let mut hasher = FingerprintHasher::with_keys(k0, k1);
        hasher.write_u8(7);
        hasher.write_usize(3);
        hasher.write(b"abcdefghij");
        hasher.write_u32(u32::MAX);
        hasher.write_u128(u128::MAX - 1);
        let words = [
            7,
            3,
            10,
            u64::from_le_bytes(*b"abcdefgh"),
            u64::from(u16::from_le_bytes(*b"ij")),
            u64::from(u32::MAX),
            u64::MAX - 1,
            u64::MAX,
        ];
        assert_eq!(hasher.finish(), siphash_1_3(k0, k1, &message(&words)));
        assert_eq!(
            FingerprintHasher::with_keys(k0, k1).finish(),
            siphash_1_3(k0, k1, &[])
        );
    }

    #[test]
    fn the_key_and_the_write_boundaries_both_reach_the_fingerprint() {
        let hash = |k0, write: &dyn Fn(&mut FingerprintHasher)| {
            let mut hasher = FingerprintHasher::with_keys(k0, 1);
            write(&mut hasher);
            hasher.finish()
        };
        let joined = |hasher: &mut FingerprintHasher| hasher.write(b"ab");
        let split = |hasher: &mut FingerprintHasher| {
            hasher.write(b"a");
            hasher.write(b"b");
        };
        assert_ne!(hash(0, &joined), hash(0, &split));
        assert_ne!(hash(0, &joined), hash(2, &joined));
    }

    /// Trailing NUL bytes are content, not word padding: under one key,
    /// strings that differ only in trailing NULs (as cooked template quasis
    /// can) never share a fingerprint, alone or inside a string list.
    #[test]
    fn trailing_nul_bytes_reach_the_fingerprint() {
        use std::hash::Hash;
        use std::sync::Arc;
        let fingerprint = |value: &dyn Fn(&mut FingerprintHasher)| {
            let mut hasher = FingerprintHasher::with_keys(3, 5);
            value(&mut hasher);
            hasher.finish()
        };
        assert_ne!(
            fingerprint(&|hasher| "a".hash(hasher)),
            fingerprint(&|hasher| "a\0".hash(hasher))
        );
        let mut fingerprints = std::collections::HashSet::new();
        for first in 0..8 {
            for second in 0..8 {
                let quasis: Arc<[Arc<str>]> = Arc::from(vec![
                    Arc::<str>::from(format!("a{}", "\0".repeat(first))),
                    Arc::<str>::from(format!("b{}", "\0".repeat(second))),
                ]);
                fingerprints.insert(fingerprint(&|hasher| quasis.hash(hasher)));
            }
        }
        assert_eq!(fingerprints.len(), 64);
    }
}
