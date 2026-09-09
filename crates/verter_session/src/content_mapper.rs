//! The content-mapper projection plane: one carrier surface's projection into
//! mapping products, plus the wire semantics for asking that projection a
//! position question.
//!
//! # What this plane owns, and what it does not
//!
//! The geometry itself belongs to the transform that emitted the bytes;
//! [`MappingProduct`] is minted there and nowhere else. This plane owns the
//! layer above it: which carrier unit a product is the projection OF, which map
//! construction it came from, and what a projected or carrier position is
//! allowed to be answered with. It resolves nothing semantic — no engine, no
//! type, no symbol — and it is reachable without one running.
//!
//! # Projection topology
//!
//! A projection is RETAINED as owned geometry, addressed by the carrier unit's
//! lineage identity, and answered by searching the compact region partition the
//! product already publishes.
//!
//! The alternative topology — re-deriving the correspondence from the
//! [`CodeTransform`](verter_compiler::code_transform::CodeTransform) on every
//! question — is not retained beside it, and is not expressible here: this type
//! carries no lifetime parameter and no constructor that borrows a transform,
//! so a caller cannot hold geometry that outlives the arena the transform was
//! built in, and cannot pay the whole chunk walk per position. The third
//! candidate, a dense per-projected-byte table, is likewise absent: it answers
//! in the same `O(1)`-ish time the partition search already achieves at
//! `O(log regions)`, while costing memory linear in the size of every generated
//! surface. Retained-and-compact is the only shape that satisfies both
//! constraints the contract places on this plane at once — answerable with the
//! engine down (so it cannot depend on live transform state) and bounded in
//! what it keeps (so it cannot scale with file size).
//!
//! # Position semantics: refusal is an answer, clamping is not
//!
//! Every query here is total: it returns a correspondence or a typed
//! [`Refusal`], and never an approximation. An offset past the end of a surface
//! is [`Refusal::OutOfRange`], NOT the last addressable offset — a clamped
//! offset is indistinguishable from a real one at the call site, so it applies
//! an edit at the wrong place instead of declining to apply it. A range that
//! covers projected bytes with no carrier preimage is refused whole, NOT
//! narrowed to the mapped part of itself, because the caller asked about the
//! range it named.
//!
//! # Lineage is not content
//!
//! [`ContentMapper::unit`] is minted from the carrier's lineage and survives
//! every edit; [`ContentMapper::revision`] is the exact map construction and
//! does not. Keying the projection on content instead would mint a new unit on
//! the first keystroke and lose every association the previous projection held.
//!
//! # Dormant
//!
//! No production route reaches this module. Activation is a separate, atomic
//! step that replaces the route it displaces rather than answering beside it.

use verter_compiler::code_transform::{
    CarrierClass, InsertionAnchor, MappingProduct, MappingSpan, ProjectedClass,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{ContentId, SourceUnitId};
use verter_identity::mapping::{MapRevision, SourceProjectionMapId};

/// Why a position question has no answer. Closed: each variant is a distinct
/// disposition a caller must be able to branch on, and none of them is a
/// degraded form of a correspondence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Refusal {
    /// The offset or range lies outside the surface this map accounts for.
    /// Includes the one-past-the-end offset, which no region covers: an
    /// insertion there is answerable only through a declared anchor.
    OutOfRange,
    /// `end` precedes `start`. A caller that inverted a range asked no
    /// question; it is not silently normalised.
    InvertedRange,
    /// The projected bytes are [`ProjectedClass::Synthesized`] — they have no
    /// carrier preimage at all.
    NoCarrierPreimage,
    /// The carrier bytes are [`CarrierClass::Elided`] — they reach no output.
    NoProjection,
    /// The covered region is [`ProjectedClass::Rewritten`], whose
    /// correspondence is region-to-region, and the request named a strict
    /// sub-range of it. Mapping it byte-to-byte would be a guess.
    NotByteAddressable,
    /// The covered regions' carrier preimages do not form one contiguous span.
    /// Answering with their hull would claim the bytes between them.
    Discontiguous,
}

/// The carrier answer to a question asked in projected coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierAnswer {
    /// A carrier POINT: the projected bytes stand at this authored offset
    /// without accounting for any authored extent.
    Point(u32),
    /// A non-empty carrier span the projected range corresponds to.
    Span(MappingSpan),
    Refused(Refusal),
}

/// The projected answer to a question asked in carrier coordinates. The
/// complete variant carries EVERY projection of the position, in ascending
/// projected order — a caller acting on fewer than all of them would leave the
/// projected surface internally inconsistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionAnswer {
    Complete(Vec<MappingSpan>),
    Refused(Refusal),
}

/// Descriptor the map-construction identity hashes through. Content and
/// geometry both enter it: two projections of the same bytes under different
/// codegen are different map constructions.
struct MapConstruction<'a> {
    unit: &'a SourceUnitId,
    content: &'a ContentId,
    product: &'a MappingProduct,
}

impl CanonicalEncode for MapConstruction<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.content_mapper.map_construction.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, self.unit.digest().as_bytes());
        encoder.field_bytes(2, self.content.digest().as_bytes());
        encoder.field_u32(3, self.product.projected_len());
        encoder.field_u32(4, self.product.carrier_len());
        // The partition itself, in its published order. A projection whose
        // regions moved is a different construction even at identical
        // endpoints, so the shape is encoded rather than summarised by its
        // length.
        let mut shape = CanonicalEncoder::new("verter.session.content_mapper.partition.v1");
        for (index, region) in self.product.projected().iter().enumerate() {
            let mut bytes = Vec::with_capacity(20);
            bytes.extend_from_slice(&(index as u32).to_le_bytes());
            bytes.extend_from_slice(&region.generated.start.to_le_bytes());
            bytes.extend_from_slice(&region.generated.end.to_le_bytes());
            bytes.push(projected_discriminant(region.class));
            match region.carrier {
                Some(span) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&span.start.to_le_bytes());
                    bytes.extend_from_slice(&span.end.to_le_bytes());
                }
                None => bytes.push(0),
            }
            shape.field_bytes(1, &bytes);
        }
        for anchor in self.product.insertion_anchors() {
            let mut bytes = Vec::with_capacity(8);
            bytes.extend_from_slice(&anchor.projected.to_le_bytes());
            bytes.extend_from_slice(&anchor.carrier.to_le_bytes());
            shape.field_bytes(2, &bytes);
        }
        encoder.field_bytes(5, shape.digest().as_bytes());
    }
}

/// Descriptor the companion-facing map identity hashes through. It names WHICH
/// unit and WHICH construction, so it is a strictly coarser statement than
/// [`MapConstruction`] rather than a second spelling of it.
struct ProjectionMapDescriptor<'a> {
    unit: &'a SourceUnitId,
    revision: &'a MapRevision,
}

impl CanonicalEncode for ProjectionMapDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.content_mapper.projection_map.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, self.unit.digest().as_bytes());
        encoder.field_bytes(2, self.revision.digest().as_bytes());
    }
}

fn projected_discriminant(class: ProjectedClass) -> u8 {
    match class {
        ProjectedClass::Identity => 0,
        ProjectedClass::Relocated => 1,
        ProjectedClass::Rewritten => 2,
        ProjectedClass::Synthesized => 3,
    }
}

/// One carrier surface's projection, and the wire semantics for asking it a
/// position question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentMapper {
    unit: SourceUnitId,
    revision: MapRevision,
    projection_map_id: SourceProjectionMapId,
    product: MappingProduct,
}

impl ContentMapper {
    /// Project a carrier surface. `unit` is the carrier's lineage identity and
    /// `content` the committed bytes the product was derived from; the product
    /// is taken by value because this plane retains geometry rather than
    /// borrowing the transform that produced it.
    #[must_use]
    pub fn project(unit: SourceUnitId, content: &ContentId, product: MappingProduct) -> Self {
        let revision = MapRevision::from_canonical(&MapConstruction {
            unit: &unit,
            content,
            product: &product,
        });
        let projection_map_id = SourceProjectionMapId::from_canonical(&ProjectionMapDescriptor {
            unit: &unit,
            revision: &revision,
        });
        Self {
            unit,
            revision,
            projection_map_id,
            product,
        }
    }

    /// The carrier unit this is the projection of. Stable across edits.
    #[must_use]
    pub fn unit(&self) -> &SourceUnitId {
        &self.unit
    }

    /// The exact map-construction identity. Changes when the content or the
    /// projection changes; never folded into [`Self::unit`].
    #[must_use]
    pub fn revision(&self) -> &MapRevision {
        &self.revision
    }

    /// The identity of the map a provider companion needs to be interpreted.
    #[must_use]
    pub fn projection_map_id(&self) -> &SourceProjectionMapId {
        &self.projection_map_id
    }

    /// The retained geometry, read-only. Exposed because this plane projects
    /// the product rather than hiding it; there is no second correspondence
    /// here for a caller to disagree with.
    #[must_use]
    pub fn product(&self) -> &MappingProduct {
        &self.product
    }

    /// Declared zero-width edit anchors, ascending by projected offset.
    #[must_use]
    pub fn insertion_anchors(&self) -> &[InsertionAnchor] {
        self.product.insertion_anchors()
    }

    /// Answer a projected range in carrier coordinates.
    ///
    /// A zero-width range is an insertion POINT and resolves through a declared
    /// anchor first, then through the exact byte delta of an
    /// [`Identity`](ProjectedClass::Identity) or
    /// [`Relocated`](ProjectedClass::Relocated) region. A non-empty range
    /// resolves only when every projected byte it names has a preimage and
    /// those preimages form one contiguous carrier span.
    #[must_use]
    pub fn to_carrier(&self, projected: MappingSpan) -> CarrierAnswer {
        if projected.end < projected.start {
            return CarrierAnswer::Refused(Refusal::InvertedRange);
        }
        if projected.end > self.product.projected_len() {
            return CarrierAnswer::Refused(Refusal::OutOfRange);
        }
        if projected.is_empty() {
            return self.point_to_carrier(projected.start);
        }
        self.span_to_carrier(projected)
    }

    fn point_to_carrier(&self, offset: u32) -> CarrierAnswer {
        let anchors = self.product.insertion_anchors();
        let first = anchors.partition_point(|anchor| anchor.projected < offset);
        let declared = &anchors[first..];
        let declared = &declared[..declared.partition_point(|anchor| anchor.projected == offset)];
        match declared {
            [] => {}
            [anchor] => return CarrierAnswer::Point(anchor.carrier),
            // Two authored destinations declared for one projected offset name
            // no single destination. Picking either is the arbitrary choice
            // this plane exists to refuse.
            _ => return CarrierAnswer::Refused(Refusal::Discontiguous),
        }
        let Some(region) = self.product.projected_at(offset) else {
            // The one-past-the-end offset, and anything beyond it, is covered
            // by no region. Without a declared anchor there is no authored
            // destination to name.
            return CarrierAnswer::Refused(Refusal::OutOfRange);
        };
        let Some(carrier) = region.carrier else {
            return CarrierAnswer::Refused(Refusal::NoCarrierPreimage);
        };
        match region.class {
            ProjectedClass::Identity | ProjectedClass::Relocated => {
                CarrierAnswer::Point(carrier.start + (offset - region.generated.start))
            }
            // Region-to-region: an interior point of rewritten text has no
            // exact authored offset, and the nearest one would be a guess.
            ProjectedClass::Rewritten => {
                if offset == region.generated.start {
                    CarrierAnswer::Point(carrier.start)
                } else {
                    CarrierAnswer::Refused(Refusal::NotByteAddressable)
                }
            }
            ProjectedClass::Synthesized => CarrierAnswer::Refused(Refusal::NoCarrierPreimage),
        }
    }

    fn span_to_carrier(&self, projected: MappingSpan) -> CarrierAnswer {
        let regions = self.product.projected();
        // The partition is ordered and gap-free, so the first region the range
        // touches is a search rather than a scan.
        let first = regions.partition_point(|region| region.generated.end <= projected.start);
        let mut contributions: Vec<MappingSpan> = Vec::new();
        for region in &regions[first..] {
            if region.generated.start >= projected.end {
                break;
            }
            let Some(carrier) = region.carrier else {
                return CarrierAnswer::Refused(Refusal::NoCarrierPreimage);
            };
            match region.class {
                ProjectedClass::Identity | ProjectedClass::Relocated => {
                    let lo = projected.start.max(region.generated.start) - region.generated.start;
                    let hi = projected.end.min(region.generated.end) - region.generated.start;
                    contributions.push(MappingSpan {
                        start: carrier.start + lo,
                        end: carrier.start + hi,
                    });
                }
                ProjectedClass::Rewritten => {
                    if projected.start > region.generated.start
                        || projected.end < region.generated.end
                    {
                        return CarrierAnswer::Refused(Refusal::NotByteAddressable);
                    }
                    contributions.push(carrier);
                }
                ProjectedClass::Synthesized => {
                    return CarrierAnswer::Refused(Refusal::NoCarrierPreimage)
                }
            }
        }
        if contributions.is_empty() {
            return CarrierAnswer::Refused(Refusal::OutOfRange);
        }
        // Relocation means projected order is not carrier order, so the union
        // is taken over the sorted preimages. A gap between two of them is a
        // run of carrier bytes this range does not name.
        contributions.sort_unstable();
        let mut hull = contributions[0];
        for span in &contributions[1..] {
            if span.start > hull.end {
                return CarrierAnswer::Refused(Refusal::Discontiguous);
            }
            hull.end = hull.end.max(span.end);
        }
        if hull.is_empty() {
            // Every contribution was a carrier point: the projected bytes stand
            // at an authored position without accounting for authored extent.
            return CarrierAnswer::Point(hull.start);
        }
        CarrierAnswer::Span(hull)
    }

    /// Answer a carrier position with EVERY projection derived from it, in
    /// ascending projected order.
    ///
    /// One authored region is routinely emitted more than once, so this is the
    /// complete list rather than the first match. An elided position answers
    /// [`Refusal::NoProjection`] — never a neighbour's projection.
    #[must_use]
    pub fn to_projected(&self, carrier_offset: u32) -> ProjectionAnswer {
        let Some(region) = self.product.carrier_at(carrier_offset) else {
            return ProjectionAnswer::Refused(Refusal::OutOfRange);
        };
        if region.class == CarrierClass::Elided {
            return ProjectionAnswer::Refused(Refusal::NoProjection);
        }
        let spans: Vec<MappingSpan> = self
            .product
            .projections_at_carrier(carrier_offset)
            .into_iter()
            .map(|projection| projection.generated)
            .collect();
        if spans.is_empty() {
            return ProjectionAnswer::Refused(Refusal::NoProjection);
        }
        ProjectionAnswer::Complete(spans)
    }
}

// The projection plane answers with no engine running. A retained callback, a
// trait object, or a borrowed handle into a semantic session would fail one of
// these bounds, and a `'static` bound additionally rejects the re-derive-from-
// a-live-transform topology this plane does not retain.
const _: fn() = || {
    fn assert_plain_owned_projection<T: 'static + Send + Sync + Clone + Eq + std::fmt::Debug>() {}
    assert_plain_owned_projection::<ContentMapper>();
    assert_plain_owned_projection::<CarrierAnswer>();
    assert_plain_owned_projection::<ProjectionAnswer>();
    assert_plain_owned_projection::<Refusal>();
};
