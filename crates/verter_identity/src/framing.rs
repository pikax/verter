//! Shared canonical-framing primitives.
//!
//! The streaming [`CanonicalEncoder`](crate::encoding::CanonicalEncoder)
//! and the exact-size construction path
//! ([`crate::exact`](crate::exact)) are one grammar, not two serializers
//! kept in agreement: every length, every header part and the set
//! canonicalization policy live here, and both paths consume them. A
//! change to this module is by construction a change to both encodings.

use crate::encoding::CanonicalDigest;

/// A field record is `u16 tag LE, u64 payload_length LE, payload`.
pub(crate) const TAG_BYTES: usize = 2;
pub(crate) const LEN_BYTES: usize = 8;

/// A digest payload is a fixed 32 bytes, which is what lets a digest set
/// canonicalize inside a pre-sized buffer instead of scratch storage.
pub(crate) const DIGEST_BYTES: usize = 32;

/// A set payload is `u64 element_count LE` then each element as
/// `u64 length LE + bytes`.
pub(crate) const SET_COUNT_BYTES: usize = 8;

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

/// One length-prefixed set element.
#[inline]
pub(crate) fn set_element_len(element_len: usize) -> usize {
    LEN_BYTES + element_len
}

/// Total set payload length for `count` fixed-size elements.
#[inline]
pub(crate) fn set_payload_len(count: usize, element_len: usize) -> usize {
    SET_COUNT_BYTES + count * set_element_len(element_len)
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

    /// Header parts in wire order. The streaming finish, the shared-slice
    /// finish and the in-place digest all hash/copy these same parts.
    pub(crate) fn parts<'a>(&'a self, domain_tag: &'a str) -> [&'a [u8]; 3] {
        [
            &self.domain_len[..],
            domain_tag.as_bytes(),
            &self.field_count[..],
        ]
    }

    /// Writes the header at the front of `out` and returns its length.
    pub(crate) fn write_into(&self, domain_tag: &str, out: &mut [u8]) -> usize {
        let mut at = 0;
        for part in self.parts(domain_tag) {
            out[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        at
    }
}

/// Visits `elements` in ascending byte order, once per distinct element.
///
/// This is the set canonicalization policy — sorted, then
/// deduplicated — shared by every encoding path. Fixed-size elements
/// make it expressible without scratch storage: each round selects the
/// least element greater than the last visited one, so repeats are
/// skipped and order is canonical in `O(elements × distinct)` with no
/// allocation. The measuring pass and the writing pass both count and
/// emit through this one routine, so they cannot disagree on which
/// elements a set contributes.
pub(crate) fn visit_sorted_distinct_digests<F>(elements: &[CanonicalDigest], visit: &mut F)
where
    F: FnMut(&CanonicalDigest),
{
    let mut last: Option<&CanonicalDigest> = None;
    loop {
        let mut next: Option<&CanonicalDigest> = None;
        for element in elements {
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
                visit(element);
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
pub(crate) fn distinct_digest_count(elements: &[CanonicalDigest]) -> usize {
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
        visit_sorted_distinct_digests(&empty, &mut |element| visited.push(element.as_bytes()[0]));
        assert!(visited.is_empty());

        let elements = [digest(5), digest(1), digest(5), digest(9), digest(1)];
        visit_sorted_distinct_digests(&elements, &mut |element| {
            visited.push(element.as_bytes()[0])
        });
        let mut expected: Vec<u8> = elements.iter().map(|d| d.as_bytes()[0]).collect();
        expected.sort();
        expected.dedup();
        assert_eq!(visited, expected);
    }

    /// The header parts a streaming finish writes, a shared-slice finish
    /// copies and an in-place digest hashes are the same bytes in the
    /// same order.
    #[test]
    fn header_layout_writes_and_reports_the_same_parts() {
        let layout = HeaderLayout::new("hdr-test", 3);
        let mut written = [0u8; 32];
        let len = layout.write_into("hdr-test", &mut written);
        let expected_len = header_len("hdr-test");
        assert_eq!(len, expected_len);
        let mut expected = Vec::new();
        for part in layout.parts("hdr-test") {
            expected.extend_from_slice(part);
        }
        assert_eq!(&written[..len], &expected[..]);
        assert_eq!(total_len("hdr-test", 7), expected_len + 7);
    }
}
