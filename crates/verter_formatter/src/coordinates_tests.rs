//! Behavior tests for the formatter byte coordinate domains.

use std::any::TypeId;

use crate::coordinates::{
    AuthoredFormatByteOffset, AuthoredFormatByteRange, CoordinateError, FormattedByteOffset,
    FormattedByteRange,
};

/// `a` (1 byte), `é` (2 bytes), `\r\n` (2 bytes), `€` (3 bytes): 8 bytes, 5
/// chars. Byte positions below are exact indexes into this text.
const MIXED_SOURCE: &str = "aé\r\n€";

fn authored(start: u32, end: u32) -> Result<AuthoredFormatByteRange, CoordinateError> {
    AuthoredFormatByteRange::for_source(
        MIXED_SOURCE,
        AuthoredFormatByteOffset::from_raw(start),
        AuthoredFormatByteOffset::from_raw(end),
    )
}

fn formatted(source: &str, start: u32, end: u32) -> Result<FormattedByteRange, CoordinateError> {
    FormattedByteRange::for_source(
        source,
        FormattedByteOffset::from_raw(start),
        FormattedByteOffset::from_raw(end),
    )
}

#[test]
fn the_four_domains_are_distinct_nominal_types() {
    assert_ne!(
        TypeId::of::<AuthoredFormatByteOffset>(),
        TypeId::of::<FormattedByteOffset>()
    );
    assert_ne!(
        TypeId::of::<AuthoredFormatByteRange>(),
        TypeId::of::<FormattedByteRange>()
    );
    assert_ne!(
        TypeId::of::<AuthoredFormatByteOffset>(),
        TypeId::of::<u32>()
    );
    assert_ne!(TypeId::of::<FormattedByteOffset>(), TypeId::of::<u32>());
}

#[test]
fn inverted_ranges_are_rejected_in_both_domains() {
    assert_eq!(
        AuthoredFormatByteRange::new(
            AuthoredFormatByteOffset::from_raw(5),
            AuthoredFormatByteOffset::from_raw(3),
        ),
        Err(CoordinateError::Inverted { start: 5, end: 3 })
    );
    assert_eq!(
        FormattedByteRange::new(
            FormattedByteOffset::from_raw(7),
            FormattedByteOffset::from_raw(2),
        ),
        Err(CoordinateError::Inverted { start: 7, end: 2 })
    );
    assert_eq!(
        authored(4, 2),
        Err(CoordinateError::Inverted { start: 4, end: 2 })
    );
}

#[test]
fn out_of_bounds_construction_is_refused_without_clamping() {
    assert_eq!(
        authored(0, 9),
        Err(CoordinateError::OutOfBounds {
            offset: 9,
            source_len: 8
        })
    );
    assert_eq!(
        authored(9, 10),
        Err(CoordinateError::OutOfBounds {
            offset: 10,
            source_len: 8
        })
    );
    assert_eq!(
        formatted("ab", 0, 3),
        Err(CoordinateError::OutOfBounds {
            offset: 3,
            source_len: 2
        })
    );
}

#[test]
fn offsets_inside_a_utf8_sequence_are_refused() {
    // `é` occupies bytes 1..3; byte 2 is interior.
    assert_eq!(
        authored(2, 3),
        Err(CoordinateError::NotCharBoundary { offset: 2 })
    );
    // `€` occupies bytes 5..8; byte 6 is interior.
    assert_eq!(
        authored(5, 6),
        Err(CoordinateError::NotCharBoundary { offset: 6 })
    );
    // The formatted domain refuses interior bytes of its own rendered text.
    assert_eq!(
        formatted("€", 0, 1),
        Err(CoordinateError::NotCharBoundary { offset: 1 })
    );
}

#[test]
fn exact_byte_identities_survive_unicode_boundaries() {
    let acute = authored(1, 3).expect("é is a complete scalar at bytes 1..3");
    assert_eq!((acute.start().get(), acute.end().get()), (1, 3));
    assert_eq!(acute.len(), 2);

    let euro = authored(5, 8).expect("€ is a complete scalar at bytes 5..8");
    assert_eq!((euro.start().get(), euro.end().get()), (5, 8));
    assert_eq!(euro.len(), 3);
}

#[test]
fn exact_byte_identities_survive_crlf_and_eof_boundaries() {
    // `\r\n` is two bytes, not one terminator.
    let crlf = authored(3, 5).expect("\\r\\n occupies bytes 3..5");
    assert_eq!((crlf.start().get(), crlf.end().get()), (3, 5));
    assert_eq!(crlf.len(), 2);

    // An empty range parked exactly at EOF is addressable.
    let eof = authored(8, 8).expect("EOF is a valid boundary");
    assert!(eof.is_empty());
    assert_eq!(eof.len(), 0);

    // The full extent is 8 bytes even though the text is 5 chars, and a
    // range ending at EOF keeps the exact byte length.
    let full = authored(0, 8).expect("the whole source is in bounds");
    assert_eq!(full.len(), 8);
    let tail = authored(5, 8).expect("the € tail ends at EOF");
    assert_eq!(tail.len(), 3);
}

#[test]
fn the_formatted_domain_tracks_its_own_rendered_text() {
    // `€a\r\n`: € occupies bytes 0..3, then `a`, `\r`, `\n` bring the length
    // to 6 bytes.
    let rendered = "€a\r\n";
    let euro = formatted(rendered, 0, 3).expect("€ opens the rendered text");
    assert_eq!(euro.len(), 3);
    let rest = formatted(rendered, 3, 6).expect("the remainder ends at EOF");
    assert_eq!(rest.len(), 3);
    // The same offset numbers mean different bytes in each domain's own text:
    // rendered bytes 3..6 are `a\r\n`, while in the authored source byte 6 is
    // interior to `€` and bytes 3..5 are the CRLF pair.
    assert_eq!(authored(3, 5).expect("authored bytes 3..5 exist").len(), 2);
}
