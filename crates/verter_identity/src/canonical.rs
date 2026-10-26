//! Shared payload behind every digest-backed identity newtype.
//!
//! Retains both the compact [`CanonicalDigest`] and the full canonical
//! bytes. Equality compares bytes, so a digest collision cannot make two
//! values compare equal. Hashing/ordering use the digest; equal bytes
//! always share a digest, so `Hash`/`Eq` stay consistent.

use std::sync::Arc;

use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};

/// Digest-plus-full-bytes canonical identity payload.
///
/// The bytes are shared: an identity is cloned into every record that
/// carries it (a flow demand basis, an obligation state, a cache key), and
/// a clone is a reference-count bump rather than a copy of the canonical
/// bytes. The bytes are immutable once encoded, so sharing them changes no
/// comparison.
#[derive(Clone, verter_retention::heap_size::HeapSize)]
pub struct Canonical {
    digest: CanonicalDigest,
    #[heap_size(with = "shared_bytes_heap")]
    bytes: Arc<[u8]>,
}

/// The shared canonical bytes are an immutable encoding every clone holds in
/// place of a copy: one `Arc` block, counted once per measurement.
fn shared_bytes_heap<'t>(
    bytes: &'t Arc<[u8]>,
    meter: &mut verter_retention::heap_size::HeapMeter<'_, 't>,
) -> usize {
    meter.shared_encoding_bytes(bytes)
}

impl Canonical {
    /// Builds from a finished [`CanonicalEncoder`].
    pub fn from_encoder(encoder: &CanonicalEncoder) -> Self {
        let bytes = encoder.finish_shared();
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

    /// The one allocation every clone of this identity shares, for a
    /// measurement that counts each distinct allocation once.
    pub fn shared_bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
}

impl PartialEq for Canonical {
    fn eq(&self, other: &Self) -> bool {
        // Digest short-circuits the common not-equal case; bytes decide. Two
        // handles on one allocation hold the same bytes.
        self.digest == other.digest
            && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes[..] == other.bytes[..])
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
            .then_with(|| self.bytes[..].cmp(&other.bytes[..]))
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

    /// A clone names the same bytes rather than a copy of them, and still
    /// compares, hashes and orders exactly as an independently encoded
    /// equal identity does.
    #[test]
    fn clones_share_the_encoded_bytes() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let original = Canonical::from_encodable(&Pair(5, 8));
        let clone = original.clone();
        assert!(
            std::ptr::eq(original.bytes().as_ptr(), clone.bytes().as_ptr()),
            "a clone shares the original's bytes"
        );
        let separate = Canonical::from_encodable(&Pair(5, 8));
        assert!(!std::ptr::eq(
            original.bytes().as_ptr(),
            separate.bytes().as_ptr()
        ));
        assert_eq!(clone, separate);
        let hash = |value: &Canonical| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(&clone), hash(&separate));
        assert_eq!(clone.cmp(&separate), core::cmp::Ordering::Equal);
        assert_eq!(original.bytes(), &Pair(5, 8).canonical_bytes()[..]);
    }

    /// Identities order by digest, then by their bytes as byte strings.
    #[test]
    fn ordering_follows_digest_then_bytes() {
        let mut values: Vec<Canonical> = (0..32)
            .map(|index| Canonical::from_encodable(&Pair(index, index * 3)))
            .collect();
        values.sort();
        for pair in values.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            assert!(
                (left.digest(), left.bytes()) < (right.digest(), right.bytes()),
                "{left:?} sorts before {right:?}"
            );
        }
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
