//! Tagged, length-delimited canonical encoding every typed identity hashes
//! through (`identity-encoding.md` §2):
//!
//! ```text
//! u32 domain_tag_length little-endian
//! bytes domain_tag UTF-8
//! u32 field_count little-endian
//! repeat fields in schema order:
//!     u16 field_tag little-endian
//!     u64 payload_length little-endian
//!     payload
//! ```
//!
//! The domain tag is in the header, so two descriptors that differ only in
//! tag or field order hash differently even when payloads coincide. The
//! digest is an index, not identity authority — collision-sensitive
//! callers compare full canonical bytes.
//!
//! The byte grammar itself lives in [`crate::framing`], shared with the
//! exact-size construction path ([`crate::exact`]); this type is the
//! streaming, growing-buffer encoding of it.

use crate::exact::CanonicalFieldSink;
use crate::framing;

/// Canonical-encoding builder. Callers push fields in schema order; this
/// type does not reorder them.
///
/// Fields are appended straight into one buffer in their wire layout
/// (`u16` tag, `u64` length, payload), so a descriptor of `n` fields costs
/// one growing buffer rather than one allocation per field, and finishing
/// it is one copy behind a fixed header.
#[derive(Debug, Default)]
pub struct CanonicalEncoder {
    domain_tag: &'static str,
    /// Every field so far, each as `tag LE u16, payload_length LE u64,
    /// payload`, in schema order.
    body: Vec<u8>,
    /// Fields in `body`.
    field_count: u32,
}

impl CanonicalEncoder {
    /// Start under `domain_tag` (the descriptor's stable namespace).
    pub fn new(domain_tag: &'static str) -> Self {
        Self {
            domain_tag,
            body: Vec::new(),
            field_count: 0,
        }
    }

    /// Append one raw field. Do not reuse `tag` for a different meaning
    /// in the same domain — bump the domain tag or compatibility epoch.
    pub fn field_bytes(&mut self, tag: u16, payload: &[u8]) -> &mut Self {
        self.body.reserve(framing::field_record_len(payload.len()));
        self.push_field_header(tag, payload.len());
        self.body.extend_from_slice(payload);
        self.field_count += 1;
        self
    }

    /// Pushes one field record header (`tag`, `payload_len`) — the shared
    /// field framing every raw field is written through.
    fn push_field_header(&mut self, tag: u16, payload_len: usize) {
        self.body.extend_from_slice(&tag.to_le_bytes());
        self.body
            .extend_from_slice(&(payload_len as u64).to_le_bytes());
    }

    /// A fixed-width `u64` field, little-endian.
    pub fn field_u64(&mut self, tag: u16, value: u64) -> &mut Self {
        self.field_bytes(tag, &value.to_le_bytes())
    }

    /// A fixed-width `u32` field, little-endian.
    pub fn field_u32(&mut self, tag: u16, value: u32) -> &mut Self {
        self.field_bytes(tag, &value.to_le_bytes())
    }

    /// Boolean as `0`/`1`, never the text `"true"`/`"false"`.
    pub fn field_bool(&mut self, tag: u16, value: bool) -> &mut Self {
        self.field_bytes(tag, &[u8::from(value)])
    }

    /// UTF-8 string, exact bytes, no Unicode normalization.
    pub fn field_str(&mut self, tag: u16, value: &str) -> &mut Self {
        self.field_bytes(tag, value.as_bytes())
    }

    /// A fixed-width 32-byte digest field (a [`CanonicalDigest`] payload),
    /// the shape composition identities use for predecessor keys and
    /// basis digests.
    pub fn field_digest(&mut self, tag: u16, digest: &CanonicalDigest) -> &mut Self {
        self.field_bytes(tag, digest.as_bytes())
    }

    /// Optional field: one-byte presence (`0`/`1`) plus payload when
    /// present. Omitting the field is indistinguishable from a zero-length
    /// present value, so the tag is required.
    pub fn field_option(&mut self, tag: u16, value: Option<&[u8]>) -> &mut Self {
        match value {
            None => self.field_bytes(tag, &[0u8]),
            Some(bytes) => {
                let mut payload = Vec::with_capacity(framing::option_payload_len(Some(bytes)));
                payload.push(1u8);
                payload.extend_from_slice(bytes);
                self.field_bytes(tag, &payload)
            }
        }
    }

    /// Set field: elements sorted, deduplicated, then each length-delimited
    /// (`u64` LE + bytes) so concatenation cannot fuse neighbours.
    ///
    /// Deduplication is what makes this a set rather than a sorted bag, and it
    /// is load-bearing rather than tidiness. The caller is describing which
    /// distinct elements were observed, so observing one of them twice is the
    /// same description as observing it once and has to encode identically. A
    /// bag encoding instead mints a second identity for one observation that
    /// happened to be reported with a repeat, which downstream is a second
    /// cache slot for one question.
    pub fn field_sorted_set<I, B>(&mut self, tag: u16, elements: I) -> &mut Self
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let mut sorted: Vec<Vec<u8>> = elements.into_iter().map(|b| b.as_ref().to_vec()).collect();
        sorted.sort();
        sorted.dedup();
        let mut payload = Vec::new();
        payload.extend_from_slice(&(sorted.len() as u64).to_le_bytes());
        for element in sorted {
            payload.extend_from_slice(&(element.len() as u64).to_le_bytes());
            payload.extend_from_slice(&element);
        }
        self.field_bytes(tag, &payload)
    }

    /// Map field: entries sorted by key. Duplicate keys return `None`
    /// (no last-wins merge).
    pub fn field_sorted_map<I, K, V>(&mut self, tag: u16, entries: I) -> Option<&mut Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut sorted: Vec<(Vec<u8>, Vec<u8>)> = entries
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_vec(), v.as_ref().to_vec()))
            .collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for pair in sorted.windows(2) {
            if pair[0].0 == pair[1].0 {
                return None;
            }
        }
        let mut payload = Vec::new();
        payload.extend_from_slice(&(sorted.len() as u64).to_le_bytes());
        for (key, value) in sorted {
            payload.extend_from_slice(&(key.len() as u64).to_le_bytes());
            payload.extend_from_slice(&key);
            payload.extend_from_slice(&(value.len() as u64).to_le_bytes());
            payload.extend_from_slice(&value);
        }
        Some(self.field_bytes(tag, &payload))
    }

    /// Explicit assigned discriminant — not `mem::discriminant` or
    /// declaration-order hashing.
    pub fn field_enum_discriminant(&mut self, tag: u16, discriminant: u32) -> &mut Self {
        self.field_u32(tag, discriminant)
    }

    /// Finalizes the canonical byte encoding.
    pub fn finish(&self) -> Vec<u8> {
        let header = framing::HeaderLayout::new(self.domain_tag, self.field_count);
        let mut out = Vec::with_capacity(framing::total_len(self.domain_tag, self.body.len()));
        for part in header.parts(self.domain_tag) {
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&self.body);
        out
    }

    /// Finalizes the canonical byte encoding into a shared slice: the same
    /// bytes as [`Self::finish`], built in one allocation for an identity
    /// that will be cloned.
    pub fn finish_shared(&self) -> std::sync::Arc<[u8]> {
        let header = framing::HeaderLayout::new(self.domain_tag, self.field_count);
        let len = framing::total_len(self.domain_tag, self.body.len());
        // One allocation at the final length, then each part copied whole.
        let mut shared: std::sync::Arc<[u8]> = std::iter::repeat_n(0, len).collect();
        let out = std::sync::Arc::get_mut(&mut shared).expect("a fresh slice has one handle");
        let header_len = header.write_into(self.domain_tag, out);
        out[header_len..].copy_from_slice(&self.body);
        shared
    }

    /// Finalizes and hashes the canonical bytes into a [`CanonicalDigest`],
    /// hashing the header and the fields in place rather than a finished
    /// copy of them.
    pub fn digest(&self) -> CanonicalDigest {
        let header = framing::HeaderLayout::new(self.domain_tag, self.field_count);
        let mut hasher = blake3::Hasher::new();
        for part in header.parts(self.domain_tag) {
            hasher.update(part);
        }
        hasher.update(&self.body);
        let digest = CanonicalDigest(*hasher.finalize().as_bytes());
        record_hash_call();
        digest
    }
}

/// The streaming encoder is one of the field sinks over a descriptor's
/// single field description: [`ExactCanonicalEncode`](crate::exact::ExactCanonicalEncode)'s
/// default streaming encoding drives the same `exact_fields` through
/// these methods, so both encodings share one grammar.
impl CanonicalFieldSink for CanonicalEncoder {
    fn field_u32(&mut self, tag: u16, value: u32) {
        Self::field_u32(self, tag, value);
    }

    fn field_digest(&mut self, tag: u16, digest: &CanonicalDigest) {
        Self::field_digest(self, tag, digest);
    }

    fn field_raw(&mut self, tag: u16, bytes: &[u8]) {
        Self::field_bytes(self, tag, bytes);
    }

    fn field_option(&mut self, tag: u16, value: Option<&[u8]>) {
        Self::field_option(self, tag, value);
    }

    fn field_digest_set(&mut self, tag: u16, elements: &[CanonicalDigest]) {
        let count = framing::distinct_digest_count(elements);
        let payload_len = framing::set_payload_len(count, framing::DIGEST_BYTES);
        self.body.reserve(framing::field_record_len(payload_len));
        self.push_field_header(tag, payload_len);
        self.body.extend_from_slice(&(count as u64).to_le_bytes());
        let body = &mut self.body;
        framing::visit_sorted_distinct_digests(elements, &mut |element| {
            body.extend_from_slice(&(framing::DIGEST_BYTES as u64).to_le_bytes());
            body.extend_from_slice(element.as_bytes());
        });
        self.field_count += 1;
    }
}

/// Domain-separated 256-bit digest of finished canonical bytes. An index,
/// not identity authority: collision-sensitive comparisons use the full
/// bytes. Identity newtypes retain both; see [`crate::identity`].
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, verter_retention::heap_size::HeapSize,
)]
pub struct CanonicalDigest([u8; 32]);

impl CanonicalDigest {
    /// Hashes already-finished canonical bytes. Prefer
    /// [`CanonicalEncoder::digest`] — this is exposed for callers that
    /// retain finished bytes separately (e.g. [`crate::identity::Canonical`]).
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let digest = Self(*blake3::hash(bytes).as_bytes());
        record_hash_call();
        digest
    }

    /// The raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lower-hex rendering, used by golden vector tests and `Debug`.
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl core::fmt::Debug for CanonicalDigest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CanonicalDigest({})", self.to_hex())
    }
}

/// Encode a descriptor into a [`CanonicalEncoder`] under a fixed domain
/// tag. Implementors own the domain fields; this crate owns the encoding
/// primitive and the identity wrapper.
pub trait CanonicalEncode {
    /// Fixed domain tag. Changing it is a new compatibility domain.
    const DOMAIN_TAG: &'static str;

    /// Push fields in schema order.
    fn encode_fields(&self, encoder: &mut CanonicalEncoder);

    /// Domain tag + fields.
    fn canonical_encode(&self) -> CanonicalEncoder {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        self.encode_fields(&mut encoder);
        encoder
    }

    /// Convenience: canonical bytes.
    fn canonical_bytes(&self) -> Vec<u8> {
        self.canonical_encode().finish()
    }

    /// Convenience: canonical digest.
    fn canonical_digest(&self) -> CanonicalDigest {
        self.canonical_encode().digest()
    }
}

/// Thread-scoped canonical-byte hash counting, for bounded-work proofs of
/// the construction path (`test-support` only; absent from production
/// builds). Counts every canonical-byte hash made on the calling thread:
/// both [`CanonicalDigest::of_bytes`] and [`CanonicalEncoder::digest`].
#[cfg(feature = "test-support")]
mod hash_calls {
    use std::cell::Cell;

    thread_local! {
        /// Canonical-byte hashes on this thread. A `const` initializer
        /// keeps the probe itself allocation-free, and thread scoping
        /// keeps sibling activity out of a measured window.
        static HASH_CALLS: Cell<u64> = const { Cell::new(0) };
    }

    pub(super) fn record() {
        // Hashing can occur while a thread is tearing down TLS; never
        // turn a valid hash into a panic there.
        let _ = HASH_CALLS.try_with(|calls| calls.set(calls.get().wrapping_add(1)));
    }

    pub(super) fn reset() {
        let _ = HASH_CALLS.try_with(|calls| calls.set(0));
    }

    pub(super) fn read() -> u64 {
        HASH_CALLS.try_with(Cell::get).unwrap_or(0)
    }
}

/// Records one canonical-byte hash. A no-op unless `test-support` proof
/// instrumentation is enabled.
#[cfg(not(feature = "test-support"))]
#[inline]
fn record_hash_call() {}

#[cfg(feature = "test-support")]
#[inline]
fn record_hash_call() {
    hash_calls::record();
}

/// Resets the thread's canonical-byte hash counter
/// ([`canonical_hash_calls`](Self)). `test-support` proof instrumentation
/// only.
#[cfg(feature = "test-support")]
pub fn reset_canonical_hash_calls() {
    hash_calls::reset();
}

/// Canonical-byte hashes (`CanonicalDigest::of_bytes` and
/// [`CanonicalEncoder::digest`] on them) made on this thread since the
/// last [`reset_canonical_hash_calls`]. `test-support` proof
/// instrumentation only: bounded-work proofs scope a construction window
/// by resetting before it and reading after, and sibling threads never
/// enter this thread's count.
#[cfg(feature = "test-support")]
pub fn canonical_hash_calls() -> u64 {
    hash_calls::read()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned blake3 hex of the golden layout in `golden_bytes_are_pinned`.
    const GOLDEN_EXAMPLE_DIGEST_HEX: &str =
        "91982d982d09531b56939bb74facbcb655f001d0b17b42162edd3c932b90109a";

    /// Two-field descriptor pinning the §5 golden byte layout.
    struct GoldenExample {
        name: &'static str,
        count: u64,
    }

    impl CanonicalEncode for GoldenExample {
        const DOMAIN_TAG: &'static str = "golden-example.v1";

        fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
            encoder.field_str(1, self.name).field_u64(2, self.count);
        }
    }

    #[test]
    fn golden_bytes_are_pinned() {
        let value = GoldenExample {
            name: "abc",
            count: 7,
        };
        let bytes = value.canonical_bytes();
        // domain_tag_length=17 LE, "golden-example.v1", field_count=2 LE,
        // field 1: tag=1 LE u16, len=3 LE u64, "abc";
        // field 2: tag=2 LE u16, len=8 LE u64, 7u64 LE.
        let mut expected = Vec::new();
        expected.extend_from_slice(&17u32.to_le_bytes());
        expected.extend_from_slice(b"golden-example.v1");
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.extend_from_slice(&1u16.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(b"abc");
        expected.extend_from_slice(&2u16.to_le_bytes());
        expected.extend_from_slice(&8u64.to_le_bytes());
        expected.extend_from_slice(&7u64.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn golden_digest_is_pinned() {
        let value = GoldenExample {
            name: "abc",
            count: 7,
        };
        // Pinned once via `blake3` over the golden bytes asserted by
        // `golden_bytes_are_pinned` above; a change in this hex value with
        // no accompanying domain-tag bump is a silent encoding regression.
        assert_eq!(value.canonical_digest().to_hex(), GOLDEN_EXAMPLE_DIGEST_HEX,);
    }

    /// The shared finish writes exactly the bytes the owned finish writes,
    /// for an encoder with no fields, one field, and many fields whose
    /// payloads range from empty to larger than the buffer's first growth.
    #[test]
    fn shared_finish_writes_the_owned_finish_bytes() {
        let empty = CanonicalEncoder::new("finish-test");
        assert_eq!(&*empty.finish_shared(), &empty.finish()[..]);

        let mut one = CanonicalEncoder::new("finish-test");
        one.field_str(1, "only");
        assert_eq!(&*one.finish_shared(), &one.finish()[..]);

        let mut many = CanonicalEncoder::new("finish-test");
        for tag in 0..64u16 {
            many.field_bytes(tag, &vec![tag as u8; usize::from(tag) * 7]);
        }
        let owned = many.finish();
        assert_eq!(&*many.finish_shared(), &owned[..]);
        assert_eq!(
            &owned[4 + "finish-test".len()..][..4],
            &64u32.to_le_bytes(),
            "the header counts every field"
        );
        assert_eq!(many.digest(), CanonicalDigest::of_bytes(&owned));
    }

    #[test]
    fn domain_tag_separates_otherwise_identical_payloads() {
        struct A;
        struct B;
        impl CanonicalEncode for A {
            const DOMAIN_TAG: &'static str = "domain-a";
            fn encode_fields(&self, e: &mut CanonicalEncoder) {
                e.field_u64(1, 42);
            }
        }
        impl CanonicalEncode for B {
            const DOMAIN_TAG: &'static str = "domain-b";
            fn encode_fields(&self, e: &mut CanonicalEncoder) {
                e.field_u64(1, 42);
            }
        }
        assert_ne!(A.canonical_digest(), B.canonical_digest());
    }

    #[test]
    fn sorted_set_is_construction_order_independent() {
        let mut a = CanonicalEncoder::new("set-test");
        a.field_sorted_set(1, ["c", "a", "b"]);
        let mut b = CanonicalEncoder::new("set-test");
        b.field_sorted_set(1, ["b", "c", "a"]);
        assert_eq!(a.finish(), b.finish());
    }

    /// Set semantics, not sorted-bag semantics. Order independence does not
    /// discriminate this on its own: a sorted bag is order independent too and
    /// still encodes one element observed twice differently from the same
    /// element observed once. Presence is what the field carries, so a repeat
    /// must encode identically while a genuinely additional element must not.
    #[test]
    fn sorted_set_collapses_repeats_without_collapsing_distinct_elements() {
        let encode = |elements: &[&str]| {
            let mut e = CanonicalEncoder::new("set-test");
            e.field_sorted_set(1, elements.iter().copied());
            e.finish()
        };
        assert_eq!(encode(&["a", "a"]), encode(&["a"]));
        assert_eq!(encode(&["b", "a", "b"]), encode(&["a", "b"]));
        assert_ne!(encode(&["a", "b"]), encode(&["a"]));
    }

    #[test]
    fn sorted_map_rejects_duplicate_keys() {
        let mut e = CanonicalEncoder::new("map-test");
        let result = e.field_sorted_map(1, [("k", "v1"), ("k", "v2")]);
        assert!(result.is_none());
    }

    #[test]
    fn sorted_map_is_construction_order_independent() {
        let mut a = CanonicalEncoder::new("map-test");
        a.field_sorted_map(1, [("b", "2"), ("a", "1")]);
        let mut b = CanonicalEncoder::new("map-test");
        b.field_sorted_map(1, [("a", "1"), ("b", "2")]);
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn option_present_and_absent_are_distinguishable() {
        let mut present = CanonicalEncoder::new("opt-test");
        present.field_option(1, Some(&[]));
        let mut absent = CanonicalEncoder::new("opt-test");
        absent.field_option(1, None);
        assert_ne!(present.finish(), absent.finish());
    }

    #[test]
    fn field_order_is_significant() {
        let mut a = CanonicalEncoder::new("order-test");
        a.field_u64(1, 1).field_u64(2, 2);
        let mut b = CanonicalEncoder::new("order-test");
        b.field_u64(2, 2).field_u64(1, 1);
        assert_ne!(a.finish(), b.finish());
    }
}
