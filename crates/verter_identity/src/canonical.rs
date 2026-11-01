//! Shared payload behind every digest-backed identity newtype.
//!
//! Retains both the compact [`CanonicalDigest`] and the full canonical
//! bytes. Equality compares bytes, so a digest collision cannot make two
//! values compare equal. Hashing/ordering use the digest; equal bytes
//! always share a digest, so `Hash`/`Eq` stay consistent.

use std::sync::Arc;

use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use crate::exact::{ExactCanonicalEncode, SizingSink, WriterSink};
use crate::framing;

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

    /// Builds from a borrowed [`ExactCanonicalEncode`] descriptor in one
    /// allocating call and one canonical-byte hash.
    ///
    /// The descriptor's fields are described once (through
    /// [`ExactCanonicalEncode::exact_fields`]); the construction measures
    /// that description, allocates the retained storage at the exact
    /// finished length, writes the header and every field — digest sets
    /// canonicalized (sorted, deduplicated) in place — and hashes the
    /// finished bytes once. There is no growing body buffer and no
    /// encoding scratch, so the total work is bounded to that single
    /// allocation and single hash, digest-set processing included. The
    /// bytes are exactly the streaming grammar's: the passes share their
    /// framing with [`CanonicalEncoder`].
    ///
    /// # Panics
    ///
    /// Descriptors must be deterministic per borrow: `exact_fields` is driven
    /// once to size and once to write. Passes that disagree on the body length
    /// or the field count panic rather than producing wrong bytes.
    pub fn from_exact<T: ExactCanonicalEncode + ?Sized>(value: &T) -> Self {
        let mut sizing = SizingSink::new();
        value.exact_fields(&mut sizing);

        // The one allocating call: retained storage at the exact finished
        // length.
        let domain_tag = <T as ExactCanonicalEncode>::DOMAIN_TAG;
        let len = framing::total_len(domain_tag, sizing.body_len());
        let mut shared: Arc<[u8]> = std::iter::repeat_n(0, len).collect();
        let mut out = framing::SliceOut::new(
            Arc::get_mut(&mut shared).expect("a fresh slice has one handle"),
        );

        framing::HeaderLayout::new(domain_tag, sizing.field_count()).put_into(domain_tag, &mut out);
        let mut writer = WriterSink::new(out);
        value.exact_fields(&mut writer);
        assert!(
            writer.written() == len && writer.field_count() == sizing.field_count(),
            "an exact-size descriptor described different fields to its sizing \
             and writing passes; exact_fields must be deterministic per borrow"
        );

        // The one canonical-byte hash.
        let digest = CanonicalDigest::of_bytes(&shared);
        Self {
            digest,
            bytes: shared,
        }
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

    struct Wide(Vec<u8>);
    impl CanonicalEncode for Wide {
        const DOMAIN_TAG: &'static str = "canonical-test.wide.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_bytes(1, &self.0);
        }
    }

    /// Two independently pinned owners of clones of one identity charge its
    /// shared bytes once, and the charge lasts until the last owner releases
    /// it, whichever releases first.
    #[test]
    fn pinned_owners_of_clones_share_one_encoding_charge() {
        use verter_retention::pinned_heap::{PinContext, PinnedHeap};
        use verter_retention::{RetentionLimits, SemanticRetentionAccount};

        for first_releases_first in [false, true] {
            let context =
                PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
            let original = Canonical::from_encodable(&Wide(vec![7; 2 * 1024 * 1024]));
            let clone = original.clone();
            assert!(Arc::ptr_eq(original.shared_bytes(), clone.shared_bytes()));
            let payload = original.bytes().len();

            let first = PinnedHeap::measure(&original, 0, &context);
            let once = context.account().snapshot().pinned_bytes;
            assert!(once >= payload, "the first owner charges the encoding");
            let second = PinnedHeap::measure(&clone, 0, &context);
            let both = context.account().snapshot().pinned_bytes;
            assert!(
                both < once + payload,
                "a second owner of a clone shares the encoding's charge: {once} then {both}"
            );

            let (early, late) = if first_releases_first {
                (first, second)
            } else {
                (second, first)
            };
            drop(early);
            assert!(
                context.account().snapshot().pinned_bytes >= payload,
                "the remaining owner keeps the encoding charged"
            );
            drop(late);
            assert_eq!(context.account().snapshot().total_bytes(), 0);
        }
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
