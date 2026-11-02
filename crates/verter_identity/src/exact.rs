//! Exact-size canonical construction.
//!
//! [`Canonical`](crate::canonical::Canonical) is the retained identity
//! payload. This module is the bounded construction contract behind
//! [`Canonical::from_exact`](crate::canonical::Canonical::from_exact): a
//! borrowed immutable descriptor produces its retained canonical bytes in
//! **one allocating call** (the retained storage itself) and **exactly
//! one canonical-byte hash**, with no encoding scratch — not in the
//! caller, not hidden in the encoder — because the whole encoding is
//! sized first and then written once into that single allocation,
//! digest-set canonicalization included.
//!
//! The grammar is not duplicated here. A descriptor describes its fields
//! once, through [`CanonicalFieldSink`]; the streaming [`CanonicalEncoder`],
//! the measuring pass and the writing pass are three sinks over that one
//! description, and every length and framing byte they produce comes from the
//! crate's private framing module. An [`ExactCanonicalEncode`] descriptor
//! receives [`CanonicalEncode`] from that same description and cannot supply
//! a second one (it would not compile), so its streaming and exact bytes
//! cannot drift apart.
//!
//! ```text
//! Canonical::from_exact(&descriptor)
//!   1. measuring pass: value.exact_fields(..) -> body length, field count
//!   2. one allocation at the exact finished length
//!   3. writing pass:   value.exact_fields(..) -> header + fields written in place
//!   4. one hash over the finished bytes
//! ```
//!
//! A digest set is supplied as a re-iterable source of digests — any `Clone`
//! iterator, so a contiguous `&[CanonicalDigest]` through `.iter().copied()`
//! or a borrowed slice of retained identity newtypes projected through their
//! `digest()` accessors — and is never collected into a digest slice first:
//! that copy would be the hidden encoding scratch this path exists to
//! remove. Digest sets cost `O(elements × distinct)` comparisons per pass
//! because they canonicalize without scratch storage, so only prepared,
//! bounded sets belong on this path. Descriptors outside this contract keep
//! the general streaming API, whose `field_sorted_set` sorts through
//! temporary storage.
//!
//! # Example
//!
//! A stage-shaped descriptor with the field kinds composition identities
//! use — a set of retained predecessor keys projected through their
//! digests, a `u32` compatibility epoch, digest and raw-byte fields,
//! optional option bytes, and an unordered, possibly repeated digest
//! set — all borrowed:
//!
//! ```
//! use verter_identity::canonical::Canonical;
//! use verter_identity::digest_identity;
//! use verter_identity::encoding::{CanonicalDigest, CanonicalEncode};
//! use verter_identity::exact::{CanonicalFieldSink, ExactCanonicalEncode};
//!
//! digest_identity!(SourceKey);
//! digest_identity!(StageKey);
//!
//! struct StageDescriptor<'a> {
//!     sources: &'a [SourceKey],
//!     epoch: u32,
//!     predecessor: &'a CanonicalDigest,
//!     options: Option<&'a [u8]>,
//!     payload: &'a [u8],
//!     demand: &'a [CanonicalDigest],
//! }
//!
//! impl ExactCanonicalEncode for StageDescriptor<'_> {
//!     const DOMAIN_TAG: &'static str = "example.stage.v1";
//!
//!     fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
//!         // Retained keys projected through their digests: nothing is
//!         // collected into a digest slice first.
//!         sink.field_digest_set(1, self.sources.iter().map(SourceKey::digest));
//!         sink.field_u32(2, self.epoch);
//!         sink.field_digest(3, self.predecessor);
//!         sink.field_option(4, self.options);
//!         sink.field_raw(5, self.payload);
//!         sink.field_digest_set(6, self.demand.iter().copied());
//!     }
//! }
//!
//! struct Source(&'static str);
//!
//! impl ExactCanonicalEncode for Source {
//!     const DOMAIN_TAG: &'static str = "example.source.v1";
//!
//!     fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
//!         sink.field_str(1, self.0);
//!     }
//! }
//!
//! let sources = [
//!     SourceKey::from_exact(&Source("b")),
//!     SourceKey::from_exact(&Source("a")),
//!     SourceKey::from_exact(&Source("b")),
//! ];
//! let digest = CanonicalDigest::of_bytes(b"predecessor");
//! let demand = [CanonicalDigest::of_bytes(b"b"), CanonicalDigest::of_bytes(b"a")];
//! let descriptor = StageDescriptor {
//!     sources: &sources,
//!     epoch: 3,
//!     predecessor: &digest,
//!     options: Some(b"strict"),
//!     payload: b"raw",
//!     demand: &demand,
//! };
//!
//! // Retained payload, one allocation, one hash — and byte-identical to
//! // the streaming grammar, which the descriptor receives from the same
//! // description:
//! let key = StageKey::from_exact(&descriptor);
//! assert_eq!(key.canonical_bytes(), descriptor.canonical_bytes().as_slice());
//! ```
//!
//! Descriptors must be deterministic: `exact_fields` is driven twice
//! (once to size, once to write) and must describe the same fields, in
//! the same order, from the same borrow. The construction asserts the
//! two passes agree on the body length and the field count, so a descriptor
//! whose passes differ in either fails loudly instead of producing silently
//! wrong bytes.
//!
//! # Proving the construction bound
//!
//! The bound is deterministic work evidence, and a consumer can prove it over
//! its own real construction without a second encoder. Enable this crate's
//! `test-support` feature for the consumer's test targets only — a
//! `[dev-dependencies]` entry on this crate with `features = ["test-support"]`,
//! the way this crate's own tests enable it, so production builds carry none
//! of the instrumentation — then run the construction between
//! `encoding::reset_canonical_hash_calls` and `encoding::canonical_hash_calls`,
//! and count `alloc`, `alloc_zeroed` and `realloc` over the same window with a
//! thread-local counting `#[global_allocator]` in the test binary. Both counters
//! are scoped to the measuring thread, so sibling tests cannot enter them. This
//! crate's `tests/cases/exact_construction.rs` is the reference oracle,
//! including the barrier-fenced sibling that proves the scoping.

use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use crate::framing;

/// Field description surface shared by every encoding path.
///
/// Descriptors describe schema fields only; framing (tags, lengths, header,
/// set canonicalization) belongs to the sinks. The required methods are the
/// three framed payload shapes. The defaulted scalar methods route through
/// [`Self::field_raw`] and are the one place scalar payload bytes are
/// defined: the streaming [`CanonicalEncoder`]'s inherent scalar methods
/// delegate to them, so no second scalar encoding exists.
pub trait CanonicalFieldSink {
    /// A raw byte field, exact bytes, no transformation.
    fn field_raw(&mut self, tag: u16, bytes: &[u8]);

    /// Optional field: one-byte presence plus payload when present;
    /// absent and present-empty are distinct payloads.
    fn field_option(&mut self, tag: u16, value: Option<&[u8]>);

    /// Digest set: unordered, possibly repeated, fixed-size elements;
    /// encoded sorted and deduplicated by the shared set policy.
    ///
    /// `elements` is any re-iterable digest source — a contiguous slice
    /// through `.iter().copied()`, or a borrowed slice of retained identity
    /// newtypes projected through their `digest()` accessors. It is
    /// re-traversed by the set policy and never collected, so no caller
    /// prepares a digest slice to use it.
    fn field_digest_set<I>(&mut self, tag: u16, elements: I)
    where
        I: IntoIterator<Item = CanonicalDigest>,
        I::IntoIter: Clone;

    /// A fixed-width `u32` field, little-endian (compatibility epochs).
    fn field_u32(&mut self, tag: u16, value: u32) {
        let bytes = value.to_le_bytes();
        self.field_raw(tag, &bytes);
    }

    /// A fixed-width 32-byte digest field.
    fn field_digest(&mut self, tag: u16, digest: &CanonicalDigest) {
        self.field_raw(tag, digest.as_bytes());
    }

    /// A fixed-width `u64` field, little-endian.
    fn field_u64(&mut self, tag: u16, value: u64) {
        let bytes = value.to_le_bytes();
        self.field_raw(tag, &bytes);
    }

    /// Boolean as `0`/`1`, never the text `"true"`/`"false"`.
    fn field_bool(&mut self, tag: u16, value: bool) {
        let bytes = [u8::from(value)];
        self.field_raw(tag, &bytes);
    }

    /// UTF-8 string, exact bytes, no Unicode normalization.
    fn field_str(&mut self, tag: u16, value: &str) {
        self.field_raw(tag, value.as_bytes());
    }
}

/// A descriptor whose encoding can be sized before it is written.
///
/// Implement this for borrowed, immutable descriptors and get
/// [`CanonicalEncode`] from the blanket implementation below, which drives
/// the same [`Self::exact_fields`] description through the streaming
/// [`CanonicalEncoder`]. A separate hand-written `CanonicalEncode` conflicts
/// with that implementation and does not compile, so a descriptor cannot
/// hold two field descriptions that drift apart.
pub trait ExactCanonicalEncode {
    /// Fixed domain tag. Changing it is a new compatibility domain.
    const DOMAIN_TAG: &'static str;

    /// Describes the schema fields to `sink`. Must be deterministic per
    /// borrow: the same fields, in the same order, every call.
    fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S);
}

impl<T: ExactCanonicalEncode + ?Sized> CanonicalEncode for T {
    const DOMAIN_TAG: &'static str = <T as ExactCanonicalEncode>::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        self.exact_fields(encoder);
    }
}

/// Measuring pass: accumulates the exact body length and field count.
pub(crate) struct SizingSink {
    body_len: usize,
    field_count: u32,
}

impl SizingSink {
    pub(crate) fn new() -> Self {
        Self {
            body_len: 0,
            field_count: 0,
        }
    }

    pub(crate) fn body_len(&self) -> usize {
        self.body_len
    }

    pub(crate) fn field_count(&self) -> u32 {
        self.field_count
    }

    fn record_payload(&mut self, payload_len: usize) {
        self.body_len += framing::field_record_len(payload_len);
        self.field_count += 1;
    }
}

impl CanonicalFieldSink for SizingSink {
    fn field_raw(&mut self, _tag: u16, bytes: &[u8]) {
        self.record_payload(bytes.len());
    }

    fn field_option(&mut self, _tag: u16, value: Option<&[u8]>) {
        self.record_payload(framing::option_payload_len(value));
    }

    fn field_digest_set<I>(&mut self, _tag: u16, elements: I)
    where
        I: IntoIterator<Item = CanonicalDigest>,
        I::IntoIter: Clone,
    {
        let count = framing::distinct_digest_count(elements.into_iter());
        self.record_payload(framing::set_payload_len(count, framing::DIGEST_BYTES));
    }
}

/// Writing pass: fills the pre-sized retained storage in place, through the
/// same framing routines as the streaming encoder. Writes are bounds-checked
/// against the measuring pass's length, and
/// [`Canonical::from_exact`](crate::canonical::Canonical::from_exact) verifies
/// the written length and field count afterwards, so a descriptor whose two
/// passes disagree cannot produce silently wrong bytes.
pub(crate) struct WriterSink<'a> {
    out: framing::SliceOut<'a>,
    field_count: u32,
}

impl<'a> WriterSink<'a> {
    /// Continues writing at `out`'s position, after the header.
    pub(crate) fn new(out: framing::SliceOut<'a>) -> Self {
        Self {
            out,
            field_count: 0,
        }
    }

    /// Total bytes written, header included.
    pub(crate) fn written(&self) -> usize {
        self.out.written()
    }

    pub(crate) fn field_count(&self) -> u32 {
        self.field_count
    }
}

impl CanonicalFieldSink for WriterSink<'_> {
    fn field_raw(&mut self, tag: u16, bytes: &[u8]) {
        framing::put_raw_field(&mut self.out, tag, bytes);
        self.field_count += 1;
    }

    fn field_option(&mut self, tag: u16, value: Option<&[u8]>) {
        framing::put_option_field(&mut self.out, tag, value);
        self.field_count += 1;
    }

    fn field_digest_set<I>(&mut self, tag: u16, elements: I)
    where
        I: IntoIterator<Item = CanonicalDigest>,
        I::IntoIter: Clone,
    {
        framing::put_digest_set_field(&mut self.out, tag, elements.into_iter());
        self.field_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measuring pass and the writing pass agree on the body length
    /// for every field kind — the invariant `Canonical::from_exact`
    /// asserts and the single-allocation contract depends on.
    #[test]
    fn sizing_and_writing_agree_on_body_length() {
        let digests = [
            CanonicalDigest::of_bytes(&[1; 48]),
            CanonicalDigest::of_bytes(&[2; 48]),
            CanonicalDigest::of_bytes(&[1; 48]),
        ];

        let mut sizer = SizingSink::new();
        sizer.field_u32(1, 7);
        sizer.field_digest(2, &digests[0]);
        sizer.field_option(3, None);
        sizer.field_option(4, Some(b"ab"));
        sizer.field_raw(5, b"raw");
        sizer.field_digest_set(6, digests.iter().copied());
        sizer.field_u64(7, 300);
        sizer.field_bool(8, true);
        sizer.field_str(9, "s");

        let mut buffer = vec![0u8; sizer.body_len()];
        let mut writer = WriterSink::new(framing::SliceOut::new(&mut buffer));
        writer.field_u32(1, 7);
        writer.field_digest(2, &digests[0]);
        writer.field_option(3, None);
        writer.field_option(4, Some(b"ab"));
        writer.field_raw(5, b"raw");
        writer.field_digest_set(6, digests.iter().copied());
        writer.field_u64(7, 300);
        writer.field_bool(8, true);
        writer.field_str(9, "s");

        assert_eq!(writer.written(), sizer.body_len());
        assert_eq!(writer.field_count(), sizer.field_count());
    }

    /// Scalar payload bytes are defined once, by the sink's scalar methods,
    /// and are public wire contract: fixed-width little-endian integers, a
    /// one-byte `0`/`1` boolean, and exact UTF-8 for strings. The streaming
    /// encoder's inherent scalar methods delegate here, so this pins both
    /// paths.
    #[test]
    fn scalar_payloads_are_little_endian_fixed_width_or_exact_utf8() {
        let describe = |sink: &mut dyn FnMut(u16, &[u8])| {
            // Each scalar as the record it must frame to.
            sink(1, &0x0403_0201u32.to_le_bytes());
            sink(2, &0x0807_0605_0403_0201u64.to_le_bytes());
            sink(3, &[1]);
            sink(4, &[0]);
            sink(5, "é".as_bytes());
        };
        let mut expected = Vec::new();
        describe(&mut |tag, payload| {
            expected.extend_from_slice(&tag.to_le_bytes());
            expected.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            expected.extend_from_slice(payload);
        });

        let mut sizer = SizingSink::new();
        sizer.field_u32(1, 0x0403_0201);
        sizer.field_u64(2, 0x0807_0605_0403_0201);
        sizer.field_bool(3, true);
        sizer.field_bool(4, false);
        sizer.field_str(5, "é");
        let mut buffer = vec![0u8; sizer.body_len()];
        {
            let mut writer = WriterSink::new(framing::SliceOut::new(&mut buffer));
            writer.field_u32(1, 0x0403_0201);
            writer.field_u64(2, 0x0807_0605_0403_0201);
            writer.field_bool(3, true);
            writer.field_bool(4, false);
            writer.field_str(5, "é");
            assert_eq!(writer.written(), expected.len());
        }
        assert_eq!(
            buffer, expected,
            "payload bytes: [1,2,3,4], [1..=8], [1], [0], UTF-8"
        );
    }
}
