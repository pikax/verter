//! Identity and carrier tests for the provenance contract.

use crate::coordinates::{AuthoredFormatByteOffset, AuthoredFormatByteRange};
use crate::provenance::{AuthoredProvenance, FormatProvenanceTable, ProvenanceError, Provenanced};

/// Synthetic caller-side source-revision identity: stable for one authored
/// text revision, unequal across revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Revision(u64);

fn span(start: u32, end: u32) -> AuthoredFormatByteRange {
    AuthoredFormatByteRange::new(
        AuthoredFormatByteOffset::from_raw(start),
        AuthoredFormatByteOffset::from_raw(end),
    )
    .expect("fixture spans are ordered")
}

/// Synthetic producer role: an authored view rendered into provenance-carrying
/// segments. Stands in for the authored-view and renderer halves; the real
/// implementations must consume the same definitions.
mod producer {
    use super::{span, Revision};

    /// A rendered formatter output segment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RenderedSegment {
        pub text: &'static str,
    }

    /// An authored view: the spans of one source revision, in authored order.
    pub struct AuthoredView {
        pub revision: Revision,
        pub spans: &'static [(u32, u32)],
    }

    /// Renders `view` in authored order, minting one provenance identity per
    /// span through `table`. The table must belong to the view's own source
    /// revision.
    pub fn render(
        view: &AuthoredView,
        table: &mut crate::provenance::FormatProvenanceTable<Revision>,
    ) -> Vec<crate::provenance::Provenanced<RenderedSegment>> {
        assert_eq!(
            table.source_revision(),
            &view.revision,
            "segments render against the view's own revision"
        );
        view.spans
            .iter()
            .map(|&(start, end)| {
                let range = span(start, end);
                let id = table.bind_authored(range);
                crate::provenance::Provenanced::new(
                    RenderedSegment {
                        text: "formatted bytes",
                    },
                    crate::provenance::AuthoredProvenance::new(id, range),
                )
            })
            .collect()
    }
}

/// Synthetic consumer role: joins rendered segments against its own
/// reconstruction of the authored provenance table.
mod consumer {
    use super::{producer::RenderedSegment, Revision};
    use crate::provenance::{FormatProvenanceTable, ProvenanceError, Provenanced};

    /// Validates every segment identity against `table` under `revision`.
    /// Nothing is re-bound: an identity must resolve to the exact binding the
    /// table recorded, under the exact revision the segments were rendered
    /// against.
    pub fn join_segments(
        segments: &[Provenanced<RenderedSegment>],
        revision: &Revision,
        table: &FormatProvenanceTable<Revision>,
    ) -> Result<(), ProvenanceError> {
        for segment in segments {
            table.validate(revision, segment.provenance())?;
        }
        Ok(())
    }
}

use consumer::join_segments;
use producer::{render, AuthoredView, RenderedSegment};

const VIEW_SPANS: &[(u32, u32)] = &[(0, 4), (4, 9), (9, 12)];

fn view(revision: Revision) -> AuthoredView {
    AuthoredView {
        revision,
        spans: VIEW_SPANS,
    }
}

fn render_against(
    revision: Revision,
) -> (
    FormatProvenanceTable<Revision>,
    Vec<Provenanced<RenderedSegment>>,
) {
    let mut table = FormatProvenanceTable::new(revision);
    let segments = render(&view(revision), &mut table);
    (table, segments)
}

#[test]
fn producer_and_consumer_segments_join_through_the_shared_definitions() {
    // The producer renders against one table; the consumer independently
    // reconstructs the same view revision and joins every segment identity
    // through its own table.
    let (producer_table, segments) = render_against(Revision(7));
    let (consumer_table, _) = render_against(Revision(7));

    assert_eq!(producer_table.source_revision(), &Revision(7));
    assert_eq!(consumer_table.source_revision(), &Revision(7));
    assert_eq!(segments.len(), 3);
    join_segments(&segments, &Revision(7), &consumer_table)
        .expect("a same-revision reconstruction joins every rendered identity");

    // The carrier keeps value and binding together through transformation.
    let preserves_provenance = segments.iter().all(|segment| {
        let provenance = *segment.provenance();
        *segment.map(|segment| segment.text.len()).provenance() == provenance
    });
    assert!(preserves_provenance);
}

#[test]
fn same_revision_reconstruction_assigns_identical_identities() {
    let (first, _) = render_against(Revision(7));
    let (mut second, _) = render_against(Revision(7));

    let first_bindings: Vec<_> = first.iter().collect();
    let second_bindings: Vec<_> = second.iter().collect();
    assert_eq!(first_bindings.len(), VIEW_SPANS.len());
    assert_eq!(first_bindings, second_bindings);

    // Re-declaring a known binding with its exact range is idempotent.
    for binding in first_bindings {
        second
            .declare(binding.id(), binding.range())
            .expect("a same-revision re-declaration of the same binding holds");
    }
}

#[test]
fn duplicate_identity_with_a_different_range_is_rejected() {
    let (mut table, _) = render_against(Revision(7));
    let bound = span(0, 4);
    let id = table.bind_authored(bound);

    assert_eq!(
        table.declare(id, span(0, 5)),
        Err(ProvenanceError::DuplicateConflictingBinding {
            id,
            existing: bound,
            requested: span(0, 5),
        })
    );
    // The conflicting declaration did not rebind anything.
    assert_eq!(table.resolve(id), Ok(bound));
}

#[test]
fn unknown_identities_are_rejected() {
    // A richer view mints identities a smaller reconstruction never saw.
    let (_, rich_segments) = render_against(Revision(7));
    let mut small = FormatProvenanceTable::new(Revision(7));
    let _ = small.bind_authored(span(0, 4));

    let foreign = rich_segments[2].provenance().id();
    assert_eq!(
        small.resolve(foreign),
        Err(ProvenanceError::UnknownId { id: foreign })
    );
    assert_eq!(
        small.declare(foreign, span(9, 12)),
        Err(ProvenanceError::UnknownId { id: foreign })
    );
    // The join stops at the first identity the reconstruction never minted.
    assert!(matches!(
        join_segments(&rich_segments, &Revision(7), &small),
        Err(ProvenanceError::UnknownId { .. })
    ));
}

#[test]
fn mismatched_bindings_are_rejected() {
    let (table, _) = render_against(Revision(7));
    let id = table
        .iter()
        .next()
        .expect("the view bound at least one span")
        .id();
    let forged = AuthoredProvenance::new(id, span(1, 4));

    assert_eq!(
        table.validate(&Revision(7), &forged),
        Err(ProvenanceError::MismatchedBinding {
            id,
            table_binding: span(0, 4),
            provenance_range: span(1, 4),
        })
    );
}

#[test]
fn cross_revision_reuse_is_rejected() {
    let (table, segments) = render_against(Revision(7));
    // A different revision reconstructing the identical view shape is still a
    // different identity space: identities minted for one source revision do
    // not validate against a table (or a claimed basis) of another revision,
    // even though every binding matches.
    let (other_revision_table, _) = render_against(Revision(8));

    assert_ne!(
        table.source_revision(),
        other_revision_table.source_revision()
    );
    // The segments' own revision joins cleanly.
    join_segments(&segments, &Revision(7), &table)
        .expect("identities validate under their minting revision");
    // Claiming another revision against the minting table is rejected...
    assert_eq!(
        join_segments(&segments, &Revision(8), &table),
        Err(ProvenanceError::RevisionMismatch)
    );
    // ...and so is presenting the identities to a differently revised table.
    assert_eq!(
        join_segments(&segments, &Revision(7), &other_revision_table),
        Err(ProvenanceError::RevisionMismatch)
    );
}

#[test]
fn stale_provenance_is_rejected_under_the_current_table_revision() {
    // The destination's own revision is supplied, the ordinal and the range
    // coincide, and the stale binding must still be rejected: a binding
    // minted under revision 7 never joins a revision-8 identity space. The
    // identity itself carries its minting revision, so no caller-supplied
    // basis can launder it.
    let (old_table, old_segments) = render_against(Revision(7));
    let (mut current_table, _) = render_against(Revision(8));

    assert_ne!(old_table.source_revision(), current_table.source_revision());

    let stale = *old_segments[0].provenance();
    let current = current_table
        .iter()
        .next()
        .expect("the current revision bound at least one span");
    // Coinciding ordinal and range across revisions never make two minted
    // identities equal, and the join under the destination's own revision is
    // still a cross-revision join.
    assert_ne!(stale.id(), current.id());
    assert_eq!(stale.range(), current.range());
    assert_eq!(
        current_table.validate(current_table.source_revision(), &stale),
        Err(ProvenanceError::RevisionMismatch)
    );
    assert_eq!(
        join_segments(
            &old_segments,
            current_table.source_revision(),
            &current_table
        ),
        Err(ProvenanceError::RevisionMismatch)
    );
    // The stale identity never resolves or re-declares in the current
    // table's numbering space: this table never minted it.
    assert_eq!(
        current_table.resolve(stale.id()),
        Err(ProvenanceError::UnknownId { id: stale.id() })
    );
    assert_eq!(
        current_table.declare(stale.id(), stale.range()),
        Err(ProvenanceError::UnknownId { id: stale.id() })
    );
}
