//! The public minting boundary, exercised from outside the crate.
//!
//! An integration test compiles without this crate's test configuration, so
//! everything here is reachable the same way production consumers (the
//! authored-view and renderer modules of later formatter nodes, in this crate
//! or another) reach it: offsets and ranges enter the world only through
//! checked construction against one exact source text, and provenance only
//! through a table for one source revision.

use verter_formatter::coordinates::{
    AuthoredFormatByteOffset, AuthoredFormatByteRange, CoordinateError, FormattedByteOffset,
    FormattedByteRange,
};
use verter_formatter::provenance::{AuthoredProvenance, FormatProvenanceTable, Provenanced};

/// `a` (1 byte), `é` (2 bytes), `\r\n` (2 bytes), `€` (3 bytes): 8 bytes.
const MIXED_SOURCE: &str = "aé\r\n€";

/// A rendered segment stand-in: the carrier shape later renderer nodes use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderedSegment {
    text: &'static str,
}

#[test]
fn offsets_mint_through_checked_construction_against_the_exact_source() {
    let euro = AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 5)
        .expect("€ opens at byte 5 of the authored source");
    assert_eq!(euro.get(), 5);
    let eof = AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 8)
        .expect("EOF is an addressable boundary");
    assert_eq!(eof.get(), 8);

    // Byte 6 is interior to `€`; byte 9 is past the end. Nothing clamps.
    assert_eq!(
        AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 6),
        Err(CoordinateError::NotCharBoundary { offset: 6 })
    );
    assert_eq!(
        AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 9),
        Err(CoordinateError::OutOfBounds {
            offset: 9,
            source_len: 8
        })
    );

    // The rendered domain mints against its own text with the same checks.
    let rendered = "€a\r\n";
    let tail = FormattedByteOffset::for_source(rendered, 3).expect("a follows €");
    assert_eq!(tail.get(), 3);
    assert_eq!(
        FormattedByteOffset::for_source(rendered, 1),
        Err(CoordinateError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        FormattedByteRange::for_source(
            rendered,
            FormattedByteOffset::for_source(rendered, 3).expect("checked start"),
            FormattedByteOffset::for_source(rendered, 6).expect("checked end at EOF"),
        )
        .expect("rendered bytes 3..6 exist")
        .len(),
        3
    );
}

#[test]
fn minted_coordinates_bind_through_a_provenance_table() {
    // The full production path a later authored-view node takes: mint both
    // offsets against the exact source, form the checked range, bind it in a
    // table for one source revision, and validate the carried provenance.
    let start = AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 5).expect("€ opens the tail");
    let end = AuthoredFormatByteOffset::for_source(MIXED_SOURCE, 8).expect("EOF closes it");
    let range = AuthoredFormatByteRange::for_source(MIXED_SOURCE, start, end)
        .expect("the € tail is a valid authored range");
    assert_eq!(range.len(), 3);

    let mut table = FormatProvenanceTable::new(7u64);
    let id = table.bind_authored(range);
    let segment = Provenanced::new(
        RenderedSegment {
            text: "formatted bytes",
        },
        AuthoredProvenance::new(id, range),
    );
    table
        .validate(table.source_revision(), segment.provenance())
        .expect("the minting table validates its own binding");
    table
        .resolve(segment.provenance().id())
        .expect("the minted identity resolves to its range");
}
