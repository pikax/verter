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
//! once, through [`CanonicalFieldSink`]; the streaming
//! [`CanonicalEncoder`](crate::encoding::CanonicalEncoder), the measuring
//! pass ([`SizingSink`]) and the writing pass ([`WriterSink`]) are three
//! sinks over the same field description, and every length and header
//! part they emit comes from [`crate::framing`]. A descriptor that
//! implements [`ExactCanonicalEncode`] and does not override
//! [`CanonicalEncode::encode_fields`] therefore cannot drift from the
//! streaming grammar: it has one field description.
//!
//! ```text
//! Canonical::from_exact(&descriptor)
//!   1. SizingSink:   value.exact_fields(&mut sizer)   -> body length, field count
//!   2. one allocation at the exact finished length
//!   3. WriterSink:   value.exact_fields(&mut writer)  -> header + fields written in place
//!   4. one hash over the finished bytes
//! ```
//!
//! # Example
//!
//! A stage-shaped descriptor with the field kinds composition identities
//! use — a `u32` compatibility epoch, digest and raw-byte fields,
//! optional option bytes, and an unordered, possibly repeated digest
//! set — all borrowed:
//!
//! ```
//! use verter_identity::canonical::Canonical;
//! use verter_identity::digest_identity;
//! use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
//! use verter_identity::exact::{CanonicalFieldSink, ExactCanonicalEncode};
//!
//! struct StageDescriptor<'a> {
//!     epoch: u32,
//!     predecessor: &'a CanonicalDigest,
//!     options: Option<&'a [u8]>,
//!     payload: &'a [u8],
//!     demand: &'a [CanonicalDigest],
//! }
//!
//! impl CanonicalEncode for StageDescriptor<'_> {
//!     const DOMAIN_TAG: &'static str = "example.stage.v1";
//!     // The streaming encoding IS the shared field description; there is
//!     // no second encode_fields implementation to keep in agreement.
//!     fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
//!         self.exact_fields(encoder);
//!     }
//! }
//!
//! impl ExactCanonicalEncode for StageDescriptor<'_> {
//!     fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
//!         sink.field_u32(1, self.epoch);
//!         sink.field_digest(2, self.predecessor);
//!         sink.field_option(3, self.options);
//!         sink.field_raw(4, self.payload);
//!         sink.field_digest_set(5, self.demand);
//!     }
//! }
//!
//! digest_identity!(StageKey);
//!
//! let digest = CanonicalDigest::of_bytes(b"predecessor");
//! let demand = [CanonicalDigest::of_bytes(b"b"), CanonicalDigest::of_bytes(b"a")];
//! let descriptor = StageDescriptor {
//!     epoch: 3,
//!     predecessor: &digest,
//!     options: Some(b"strict"),
//!     payload: b"raw",
//!     demand: &demand,
//! };
//!
//! // Retained payload, one allocation, one hash — and byte-identical to
//! // the streaming grammar:
//! let key = StageKey::from_exact(&descriptor);
//! assert_eq!(key.canonical_bytes(), descriptor.canonical_bytes().as_slice());
//! ```
//!
//! Descriptors must be deterministic: `exact_fields` is driven twice
//! (once to size, once to write) and must describe the same fields, in
//! the same order, from the same borrow. The construction asserts the
//! two passes agree on the body length, so a descriptor that cannot
//! honour that fails loudly instead of producing silently wrong bytes.

use crate::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use crate::framing;

/// Field description surface shared by every encoding path.
///
/// Implementors describe schema fields only; framing (tags, lengths,
/// header, set canonicalization) belongs to the sinks and
/// [`crate::framing`]. The required methods are the field kinds
/// composition identities need; the defaulted ones route through
/// [`Self::field_raw`] so they share the same framing.
pub trait CanonicalFieldSink {
    /// A fixed-width `u32` field, little-endian (compatibility epochs).
    fn field_u32(&mut self, tag: u16, value: u32);

    /// A fixed-width 32-byte digest field.
    fn field_digest(&mut self, tag: u16, digest: &CanonicalDigest);

    /// A raw byte field, exact bytes, no transformation.
    fn field_raw(&mut self, tag: u16, bytes: &[u8]);

    /// Optional field: one-byte presence plus payload when present;
    /// absent and present-empty are distinct payloads.
    fn field_option(&mut self, tag: u16, value: Option<&[u8]>);

    /// Digest set: unordered, possibly repeated, fixed-size elements;
    /// encoded sorted and deduplicated by the shared set policy.
    fn field_digest_set(&mut self, tag: u16, elements: &[CanonicalDigest]);

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
/// [`CanonicalEncode`] for free: the default
/// [`CanonicalEncode::encode_fields`] drives the same
/// [`Self::exact_fields`] description through the streaming
/// [`CanonicalEncoder`]'s field-sink implementation, so the streaming and
/// exact-size encodings share one grammar instead of two serializers.
pub trait ExactCanonicalEncode: CanonicalEncode {
    /// Describes the schema fields to `sink`. Must be deterministic per
    /// borrow: the same fields, in the same order, every call.
    fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S);

    /// Streaming encoding from the shared field description.
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
    fn field_u32(&mut self, _tag: u16, _value: u32) {
        self.record_payload(4);
    }

    fn field_digest(&mut self, _tag: u16, _digest: &CanonicalDigest) {
        self.record_payload(framing::DIGEST_BYTES);
    }

    fn field_raw(&mut self, _tag: u16, bytes: &[u8]) {
        self.record_payload(bytes.len());
    }

    fn field_option(&mut self, _tag: u16, value: Option<&[u8]>) {
        self.record_payload(framing::option_payload_len(value));
    }

    fn field_digest_set(&mut self, _tag: u16, elements: &[CanonicalDigest]) {
        let count = framing::distinct_digest_count(elements);
        self.record_payload(framing::set_payload_len(count, framing::DIGEST_BYTES));
    }
}

/// Writing pass: fills the pre-sized buffer in place. Every write is
/// bounds-checked against the measuring pass's lengths, and
/// [`Canonical::from_exact`](crate::canonical::Canonical::from_exact)
/// verifies the written body length afterwards, so a descriptor whose two
/// passes disagree cannot produce silently truncated or padded bytes.
pub(crate) struct WriterSink<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl<'a> WriterSink<'a> {
    pub(crate) fn new(out: &'a mut [u8]) -> Self {
        Self { out, at: 0 }
    }

    /// Bytes written so far (the body length, header excluded).
    pub(crate) fn written(&self) -> usize {
        self.at
    }

    fn write(&mut self, bytes: &[u8]) {
        let (head, _) = self.out[self.at..].split_at_mut(bytes.len());
        head.copy_from_slice(bytes);
        self.at += bytes.len();
    }

    fn write_field_header(&mut self, tag: u16, payload_len: usize) {
        let (head, _) = self.out[self.at..].split_at_mut(framing::TAG_BYTES + framing::LEN_BYTES);
        head[..framing::TAG_BYTES].copy_from_slice(&tag.to_le_bytes());
        let len_bytes = (payload_len as u64).to_le_bytes();
        head[framing::TAG_BYTES..].copy_from_slice(&len_bytes);
        self.at += framing::TAG_BYTES + framing::LEN_BYTES;
    }
}

impl CanonicalFieldSink for WriterSink<'_> {
    fn field_u32(&mut self, tag: u16, value: u32) {
        let bytes = value.to_le_bytes();
        self.field_raw(tag, &bytes);
    }

    fn field_digest(&mut self, tag: u16, digest: &CanonicalDigest) {
        self.field_raw(tag, digest.as_bytes());
    }

    fn field_raw(&mut self, tag: u16, bytes: &[u8]) {
        self.write_field_header(tag, bytes.len());
        self.write(bytes);
    }

    fn field_option(&mut self, tag: u16, value: Option<&[u8]>) {
        match value {
            None => {
                self.write_field_header(tag, framing::option_payload_len(None));
                self.write(&[0u8]);
            }
            Some(bytes) => {
                self.write_field_header(tag, framing::option_payload_len(Some(bytes)));
                self.write(&[1u8]);
                self.write(bytes);
            }
        }
    }

    fn field_digest_set(&mut self, tag: u16, elements: &[CanonicalDigest]) {
        let count = framing::distinct_digest_count(elements);
        let payload_len = framing::set_payload_len(count, framing::DIGEST_BYTES);
        self.write_field_header(tag, payload_len);
        self.write(&(count as u64).to_le_bytes());
        let at = &mut self.at;
        let out = &mut *self.out;
        framing::visit_sorted_distinct_digests(elements, &mut |element| {
            // `at`/`out` are reborrowed per element; the set was sized
            // for exactly `count` elements, so writes stay in bounds.
            let len_bytes = (framing::DIGEST_BYTES as u64).to_le_bytes();
            let (head, _) = out[*at..].split_at_mut(framing::LEN_BYTES);
            head.copy_from_slice(&len_bytes);
            *at += framing::LEN_BYTES;
            let (head, _) = out[*at..].split_at_mut(framing::DIGEST_BYTES);
            head.copy_from_slice(element.as_bytes());
            *at += framing::DIGEST_BYTES;
        });
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
        sizer.field_digest_set(6, &digests);
        sizer.field_u64(7, 300);
        sizer.field_bool(8, true);
        sizer.field_str(9, "s");

        let mut buffer = vec![0u8; sizer.body_len()];
        let mut writer = WriterSink::new(&mut buffer);
        writer.field_u32(1, 7);
        writer.field_digest(2, &digests[0]);
        writer.field_option(3, None);
        writer.field_option(4, Some(b"ab"));
        writer.field_raw(5, b"raw");
        writer.field_digest_set(6, &digests);
        writer.field_u64(7, 300);
        writer.field_bool(8, true);
        writer.field_str(9, "s");

        assert_eq!(writer.written(), sizer.body_len());
    }
}
