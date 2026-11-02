//! Shared canonical-framing primitives.
//!
//! The streaming [`CanonicalEncoder`](crate::encoding::CanonicalEncoder)
//! and the exact-size construction path ([`crate::exact`]) are one grammar,
//! not two serializers kept in agreement. Every framing byte is produced by
//! the `put_*` routines here, over the [`FrameOut`] destination each path
//! supplies; every length and the set canonicalization policy live here too.
//! A change to this module is by construction a change to both encodings.

use crate::encoding::CanonicalDigest;

/// A field record is `u16 tag LE, u64 payload_length LE, payload`.
const TAG_BYTES: usize = 2;
const LEN_BYTES: usize = 8;

/// A digest payload is a fixed 32 bytes, which is what lets a digest set
/// canonicalize inside a pre-sized buffer instead of scratch storage.
pub(crate) const DIGEST_BYTES: usize = 32;

/// A set payload is `u64 element_count LE` then each element as
/// `u64 length LE + bytes`.
const SET_COUNT_BYTES: usize = 8;

/// Option presence byte: absent (`0`) and present (`1`) stay distinguishable,
/// including from a present-empty value.
const OPTION_ABSENT: u8 = 0;
const OPTION_PRESENT: u8 = 1;

/// Total length of one field record (`tag`, `length`, `payload`).
#[inline]
pub(crate) fn field_record_len(payload_len: usize) -> usize {
    TAG_BYTES + LEN_BYTES + payload_len
}

/// Option payload length: one presence byte, plus the value's bytes when
/// present. Absent (`0`) and present-empty (`1`) stay distinguishable.
#[inline]
pub(crate) fn option_payload_len(value: Option<&[u8]>) -> usize {
    match value {
        None => 1,
        Some(bytes) => 1 + bytes.len(),
    }
}

/// Total set payload length for `count` fixed-size elements.
#[inline]
pub(crate) fn set_payload_len(count: usize, element_len: usize) -> usize {
    SET_COUNT_BYTES + count * (LEN_BYTES + element_len)
}

/// Destination of framed bytes: the streaming encoder's growing buffer, or
/// the exact-size path's pre-sized retained storage.
pub(crate) trait FrameOut {
    fn put(&mut self, bytes: &[u8]);
}

impl FrameOut for Vec<u8> {
    #[inline]
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// Fills a pre-sized buffer from the front. Overrunning it panics, so a
/// writer that was sized too small cannot produce truncated bytes.
pub(crate) struct SliceOut<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl<'a> SliceOut<'a> {
    pub(crate) fn new(out: &'a mut [u8]) -> Self {
        Self { out, at: 0 }
    }

    /// Bytes written so far.
    pub(crate) fn written(&self) -> usize {
        self.at
    }
}

impl FrameOut for SliceOut<'_> {
    #[inline]
    fn put(&mut self, bytes: &[u8]) {
        self.out[self.at..self.at + bytes.len()].copy_from_slice(bytes);
        self.at += bytes.len();
    }
}

#[inline]
fn put_field_header<O: FrameOut>(out: &mut O, tag: u16, payload_len: usize) {
    out.put(&tag.to_le_bytes());
    out.put(&(payload_len as u64).to_le_bytes());
}

/// `u64 length LE` then the bytes: the delimiter that keeps neighbouring set
/// elements and map entries from fusing.
#[inline]
pub(crate) fn put_length_prefixed<O: FrameOut>(out: &mut O, bytes: &[u8]) {
    out.put(&(bytes.len() as u64).to_le_bytes());
    out.put(bytes);
}

/// A set or map payload's leading element count.
#[inline]
pub(crate) fn put_element_count<O: FrameOut>(out: &mut O, count: usize) {
    out.put(&(count as u64).to_le_bytes());
}

/// One raw field record.
#[inline]
pub(crate) fn put_raw_field<O: FrameOut>(out: &mut O, tag: u16, payload: &[u8]) {
    put_field_header(out, tag, payload.len());
    out.put(payload);
}

/// One optional field record: presence byte, plus the value when present.
#[inline]
pub(crate) fn put_option_field<O: FrameOut>(out: &mut O, tag: u16, value: Option<&[u8]>) {
    put_field_header(out, tag, option_payload_len(value));
    match value {
        None => out.put(&[OPTION_ABSENT]),
        Some(bytes) => {
            out.put(&[OPTION_PRESENT]);
            out.put(bytes);
        }
    }
}

/// One digest-set field record, canonicalized by the shared set policy.
///
/// `elements` is a re-iterable digest source: it is traversed once to count
/// the distinct elements for the record's length and then once per distinct
/// element to select it, and is never collected anywhere.
pub(crate) fn put_digest_set_field<O, I>(out: &mut O, tag: u16, elements: I)
where
    O: FrameOut,
    I: Iterator<Item = CanonicalDigest> + Clone,
{
    let count = distinct_digest_count(elements.clone());
    put_field_header(out, tag, set_payload_len(count, DIGEST_BYTES));
    put_element_count(out, count);
    visit_sorted_distinct_digests(elements, &mut |element| {
        put_length_prefixed(out, element.as_bytes());
    });
}

/// Wire header: `u32 domain_tag_length LE, domain tag, u32 field_count LE`.
pub(crate) struct HeaderLayout {
    domain_len: [u8; 4],
    field_count: [u8; 4],
}

/// Header length for a domain tag (everything before the field records).
#[inline]
pub(crate) fn header_len(domain_tag: &str) -> usize {
    4 + domain_tag.len() + 4
}

/// Total finished length for a domain tag and a body of `body_len` bytes.
#[inline]
pub(crate) fn total_len(domain_tag: &str, body_len: usize) -> usize {
    header_len(domain_tag) + body_len
}

impl HeaderLayout {
    pub(crate) fn new(domain_tag: &str, field_count: u32) -> Self {
        Self {
            domain_len: (domain_tag.len() as u32).to_le_bytes(),
            field_count: field_count.to_le_bytes(),
        }
    }

    /// Header parts in wire order. Every finish copies and the in-place
    /// digest hashes these same parts.
    pub(crate) fn parts<'a>(&'a self, domain_tag: &'a str) -> [&'a [u8]; 3] {
        [
            &self.domain_len[..],
            domain_tag.as_bytes(),
            &self.field_count[..],
        ]
    }

    /// Puts the header parts, in wire order.
    pub(crate) fn put_into<O: FrameOut>(&self, domain_tag: &str, out: &mut O) {
        for part in self.parts(domain_tag) {
            out.put(part);
        }
    }
}

/// Visits `elements` in ascending byte order, once per distinct element.
///
/// This is the set canonicalization policy — sorted, then
/// deduplicated — shared by every encoding path. Fixed-size elements
/// and a re-iterable source make it expressible without scratch storage:
/// each round re-traverses the source and selects the least element
/// greater than the last visited one, so repeats are skipped and order
/// is canonical in `O(elements × distinct)` with no allocation. The
/// source may be any `Clone` iterator of digests — a contiguous slice
/// through `.iter().copied()`, or a slice of retained identities projected
/// through their digest accessors — so a caller never copies its elements
/// into a digest slice first. The measuring pass and the writing pass
/// both count and emit through this one routine, so they cannot disagree
/// on which elements a set contributes.
fn visit_sorted_distinct_digests<I, F>(elements: I, visit: &mut F)
where
    I: Iterator<Item = CanonicalDigest> + Clone,
    F: FnMut(&CanonicalDigest),
{
    let mut last: Option<CanonicalDigest> = None;
    loop {
        let mut next: Option<CanonicalDigest> = None;
        for element in elements.clone() {
            if let Some(seen) = last {
                if element.as_bytes() <= seen.as_bytes() {
                    continue;
                }
            }
            next = match next {
                None => Some(element),
                Some(least) if element.as_bytes() < least.as_bytes() => Some(element),
                other => other,
            };
        }
        match next {
            Some(element) => {
                visit(&element);
                last = Some(element);
            }
            None => return,
        }
    }
}

/// The number of elements a set contributes: its distinct-element count.
///
/// Both the measuring pass and the writing pass count a set through this
/// one routine, so their element counts cannot disagree.
pub(crate) fn distinct_digest_count<I>(elements: I) -> usize
where
    I: Iterator<Item = CanonicalDigest> + Clone,
{
    let mut count = 0;
    visit_sorted_distinct_digests(elements, &mut |_| count += 1);
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: u8) -> CanonicalDigest {
        CanonicalDigest::of_bytes(&[seed; 48])
    }

    /// Ascending order, one visit per distinct element, empty input
    /// included. The selection routine is the shared set policy; these
    /// pin its contract independently of any one encoding path, with the
    /// expected order derived from the standard library's sort.
    #[test]
    fn sorted_distinct_visits_are_ascending_and_deduplicated() {
        let empty: [CanonicalDigest; 0] = [];
        let mut visited: Vec<u8> = Vec::new();
        visit_sorted_distinct_digests(empty.iter().copied(), &mut |element| {
            visited.push(element.as_bytes()[0])
        });
        assert!(visited.is_empty());

        let elements = [digest(5), digest(1), digest(5), digest(9), digest(1)];
        visit_sorted_distinct_digests(elements.iter().copied(), &mut |element| {
            visited.push(element.as_bytes()[0])
        });
        let mut expected: Vec<u8> = elements.iter().map(|d| d.as_bytes()[0]).collect();
        expected.sort();
        expected.dedup();
        assert_eq!(visited, expected);
    }

    /// The header a finish copies, into either destination, and the parts an
    /// in-place digest hashes are the same bytes in the same order.
    #[test]
    fn header_layout_puts_and_reports_the_same_parts() {
        let layout = HeaderLayout::new("hdr-test", 3);
        let mut expected = Vec::new();
        for part in layout.parts("hdr-test") {
            expected.extend_from_slice(part);
        }
        assert_eq!(expected.len(), header_len("hdr-test"));
        assert_eq!(total_len("hdr-test", 7), header_len("hdr-test") + 7);

        let mut growing = Vec::new();
        layout.put_into("hdr-test", &mut growing);
        assert_eq!(growing, expected);

        let mut buffer = [0u8; 32];
        let mut sized = SliceOut::new(&mut buffer);
        layout.put_into("hdr-test", &mut sized);
        let written = sized.written();
        assert_eq!(&buffer[..written], &expected[..]);
    }
}
