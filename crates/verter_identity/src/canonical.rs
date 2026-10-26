//! Shared payload behind every digest-backed identity newtype.
//!
//! Retains both the compact [`CanonicalDigest`] and the full canonical
//! bytes. Equality compares bytes, so a digest collision cannot make two
//! values compare equal. Hashing/ordering use the digest; equal bytes
//! always share a digest, so `Hash`/`Eq` stay consistent.

use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};

/// Digest-plus-full-bytes canonical identity payload.
#[derive(Clone)]
pub struct Canonical {
    digest: CanonicalDigest,
    bytes: Vec<u8>,
}

impl Canonical {
    /// Builds from a finished [`CanonicalEncoder`].
    pub fn from_encoder(encoder: &CanonicalEncoder) -> Self {
        let bytes = encoder.finish();
        let digest = CanonicalDigest::of_bytes(&bytes);
        Self { digest, bytes }
    }

    /// Builds from a [`CanonicalEncode`] value.
    pub fn from_encodable<T: CanonicalEncode>(value: &T) -> Self {
        Self::from_encoder(&value.canonical_encode())
    }

    /// The compact digest.
    pub fn digest(&self) -> CanonicalDigest {
        self.digest
    }

    /// The full canonical bytes retained for collision-safe equality.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl PartialEq for Canonical {
    fn eq(&self, other: &Self) -> bool {
        // Digest short-circuits the common not-equal case; bytes decide.
        self.digest == other.digest && self.bytes == other.bytes
    }
}
impl Eq for Canonical {}

impl core::hash::Hash for Canonical {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        // O(1) hash. Sound because equal bytes always share a digest.
        self.digest.hash(state);
    }
}

impl PartialOrd for Canonical {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Canonical {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.digest
            .cmp(&other.digest)
            .then_with(|| self.bytes.cmp(&other.bytes))
    }
}

impl core::fmt::Debug for Canonical {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Byte length only, never the full payload: canonical bytes can
        // embed arbitrary source-derived content, and this type has no
        // presentation-profile authority to render it (semantic-profile.md
        // §1 — `PresentationProfileId` owns rendered forms, this crate does
        // not).
        write!(
            f,
            "Canonical(digest={}, {} bytes)",
            self.digest.to_hex(),
            self.bytes.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pair(u64, u64);
    impl CanonicalEncode for Pair {
        const DOMAIN_TAG: &'static str = "canonical-test.pair.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_u64(1, self.0).field_u64(2, self.1);
        }
    }

    #[test]
    fn equal_descriptors_are_equal_and_same_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let a = Canonical::from_encodable(&Pair(1, 2));
        let b = Canonical::from_encodable(&Pair(1, 2));
        assert_eq!(a, b);
        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn different_descriptors_are_not_equal() {
        let a = Canonical::from_encodable(&Pair(1, 2));
        let b = Canonical::from_encodable(&Pair(1, 3));
        assert_ne!(a, b);
    }

    #[test]
    fn ordering_is_deterministic_and_total() {
        let a = Canonical::from_encodable(&Pair(1, 2));
        let b = Canonical::from_encodable(&Pair(1, 3));
        // Whichever direction, ordering must be consistent (antisymmetric)
        // and stable across repeated calls.
        let first = a.cmp(&b);
        assert_eq!(first, a.cmp(&b));
        assert_eq!(b.cmp(&a), first.reverse());
    }
}
