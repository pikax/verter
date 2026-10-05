//! The 128-bit content hash of source-side identities: XXH3-128 over the
//! input bytes, little-endian, attributed as `ContentHash`. In-process only,
//! never persisted. Distinct from the truncated SHA-256 `hash_16` of
//! `verter_session_query::analysis::types`; the two are not interchangeable.

use verter_session_query::analysis::types::Hash16;

pub(crate) fn hash_16(input: &[u8]) -> Hash16 {
    verter_audit::attribute_n!(ContentHash, input.len());
    xxhash_rust::xxh3::xxh3_128(input).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_16_deterministic_and_distinct() {
        let h1 = hash_16(b"hello");
        let h2 = hash_16(b"hello");
        let h3 = hash_16(b"world");
        assert_eq!(h1, h2, "same input should produce same hash");
        assert_ne!(h1, h3, "different inputs should produce different hashes");
        assert_ne!(
            h1, [0u8; 16],
            "hash should not be all zeros for non-empty input"
        );
    }

    #[test]
    fn hash_16_is_little_endian_xxh3_128() {
        assert_eq!(
            hash_16(b"verter"),
            xxhash_rust::xxh3::xxh3_128(b"verter").to_le_bytes()
        );
    }
}
