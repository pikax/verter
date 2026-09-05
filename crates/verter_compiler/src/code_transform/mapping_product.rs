//! The dual-surface mapping product: the total, typed correspondence between a
//! carrier source and the surface projected from it.
//!
//! # Why this lives inside `CodeTransform`
//!
//! The emission record that produced the projected bytes is the only authority
//! that knows which output region came from which authored region. Recovering
//! that correspondence afterwards — by searching, slicing, or matching the
//! generated text — reads a string whose offsets the transform already moved, so
//! it desynchronises exactly where an edit happened. The product is therefore
//! derived from the SAME chunk walk that [`CodeTransform::build_string`] uses,
//! and from nothing else.
//!
//! # Totality is stated over BOTH surfaces
//!
//! Every byte of the projected surface belongs to exactly one of
//! [`ProjectedClass::Identity`], [`Relocated`](ProjectedClass::Relocated),
//! [`Rewritten`](ProjectedClass::Rewritten), or
//! [`Synthesized`](ProjectedClass::Synthesized); every byte of the carrier
//! source belongs to exactly one of [`CarrierClass::Identity`],
//! [`Relocated`](CarrierClass::Relocated),
//! [`Rewritten`](CarrierClass::Rewritten), or [`Elided`](CarrierClass::Elided).
//!
//! Partitioning only the projected side would be a hole rather than a
//! simplification: elided carrier bytes contribute no projected byte at all, so
//! a projected-only partition leaves those carrier positions with no
//! disposition — which is precisely the "mapped to a neighbour" mis-mapping the
//! elided class exists to refuse.
//!
//! # Correspondence is one-to-many, and complete in that direction
//!
//! One authored region is routinely emitted more than once (a value expression
//! emitted as a read and again as an assignment target; a destructuring pattern
//! emitted alongside the bindings it introduces). A carrier region therefore
//! answers with EVERY projected region derived from it, in ascending projected
//! order, never with the first one found — an operation anchored at a carrier
//! region that reached only one of three emissions would leave the projected
//! surface internally inconsistent, and neither fail-closed class would catch
//! it.
//!
//! # Geometry never asks a question it cannot answer alone
//!
//! Every type here is plain owned data: byte offsets, closed enums, and index
//! lists. There is no callback, closure parameter, trait object, or handle
//! through which a semantic query could be issued, and the crate that owns this
//! product does not depend on the crate that owns a type-engine binding. That is
//! what keeps the correspondence total, deterministic under cancellation, and
//! answerable with no engine running at all. The static assertion at the bottom
//! of this file is the compile-time half of that guarantee: a field that could
//! carry an engine query would not satisfy its bounds.

use super::chunk::Chunk;
use super::code_transform::CodeTransform;

/// The class of a byte range in the PROJECTED surface. Closed and exhaustive: a
/// projected region matching no variant is a lowering defect, not a fifth class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProjectedClass {
    /// Exact carrier bytes, unmoved relative to their block. The correspondence
    /// is an offset delta.
    Identity,
    /// Exact carrier bytes emitted at a different position or nesting. The
    /// correspondence is region-to-region and order-preserving.
    Relocated,
    /// Carrier bytes whose text is transformed while one carrier region stays
    /// responsible for them. The correspondence is region-to-region, NOT
    /// byte-to-byte: a sub-region edit that cannot be mapped exactly fails
    /// closed.
    Rewritten,
    /// Projected bytes with no carrier preimage. Fail closed: nothing is
    /// surfaced at a carrier position for them, and an edit covering them is
    /// refused rather than approximated.
    Synthesized,
}

/// The class of a byte range in the CARRIER surface. Closed and exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CarrierClass {
    /// Emitted unmoved somewhere in the projected surface.
    Identity,
    /// Emitted verbatim, at a different position or nesting.
    Relocated,
    /// Emitted through transformed text one carrier region stays responsible
    /// for.
    Rewritten,
    /// Carrier bytes with no projection at all. Fail closed: a position inside
    /// an elided region has no correlate and must be reported as such, never
    /// mapped to a neighbour.
    Elided,
}

/// A half-open byte span `[start, end)`.
///
/// A ZERO-WIDTH span is meaningful rather than degenerate: it is a carrier POINT
/// that projected bytes stand at without accounting for any authored extent (a
/// resolved expression emitted for a position rather than for a range). It
/// attaches to the carrier region beginning at that point, so the
/// correspondence stays answerable from the carrier side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// One region of the projected surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedRegion {
    /// The projected byte span this region covers.
    pub generated: Span,
    pub class: ProjectedClass,
    /// The carrier preimage. `None` EXACTLY when `class` is
    /// [`ProjectedClass::Synthesized`] — the same fact stated once as a class
    /// and once as data, so a consumer that branches on either reads the same
    /// disposition.
    pub carrier: Option<Span>,
}

/// One region of the carrier surface, with every projection derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierRegion {
    /// The carrier byte span this region covers. Never empty.
    pub source: Span,
    pub class: CarrierClass,
    /// Indices into [`MappingProduct::projected`], ascending — which is
    /// projected-offset order, because the projected partition is itself built
    /// in output order. EMPTY exactly when `class` is [`CarrierClass::Elided`].
    pub projected: Vec<u32>,
}

/// A zero-width provider insertion point with an exact authored destination.
/// It does not give the surrounding synthesized bytes a carrier preimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InsertionAnchor {
    pub projected: u32,
    pub carrier: u32,
}

/// The total, typed correspondence between one carrier source and the surface
/// projected from it.
///
/// Constructed only by [`MappingProduct::of`], from a [`CodeTransform`]'s own
/// chunk walk. Both partitions are contiguous, gap-free, overlap-free, and
/// ordered by offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingProduct {
    projected: Vec<ProjectedRegion>,
    carrier: Vec<CarrierRegion>,
    insertion_anchors: Vec<InsertionAnchor>,
    projected_len: u32,
    carrier_len: u32,
}

impl MappingProduct {
    /// Derive the product from the transform's emission record.
    #[must_use]
    pub fn of(transform: &CodeTransform<'_>) -> Self {
        let (projected, insertion_anchors) = project(transform);
        let carrier_len = transform.original().len() as u32;
        let carrier = partition_carrier(&projected, carrier_len);
        let projected_len = projected.last().map_or(0, |region| region.generated.end);
        Self {
            projected,
            carrier,
            insertion_anchors,
            projected_len,
            carrier_len,
        }
    }

    /// The projected partition, in ascending projected order.
    #[must_use]
    pub fn projected(&self) -> &[ProjectedRegion] {
        &self.projected
    }

    /// The carrier partition, in ascending carrier order.
    #[must_use]
    pub fn carrier(&self) -> &[CarrierRegion] {
        &self.carrier
    }

    /// Exact zero-width edit anchors, ordered by projected offset.
    #[must_use]
    pub fn insertion_anchors(&self) -> &[InsertionAnchor] {
        &self.insertion_anchors
    }

    /// Total projected bytes the partition accounts for.
    #[must_use]
    pub fn projected_len(&self) -> u32 {
        self.projected_len
    }

    /// Total carrier bytes the partition accounts for.
    #[must_use]
    pub fn carrier_len(&self) -> u32 {
        self.carrier_len
    }

    /// The projected region containing `offset`, or `None` past the end.
    #[must_use]
    pub fn projected_at(&self, offset: u32) -> Option<&ProjectedRegion> {
        let index = self
            .projected
            .partition_point(|region| region.generated.end <= offset);
        self.projected
            .get(index)
            .filter(|region| region.generated.start <= offset)
    }

    /// The carrier region containing `offset`, or `None` past the end.
    #[must_use]
    pub fn carrier_at(&self, offset: u32) -> Option<&CarrierRegion> {
        let index = self
            .carrier
            .partition_point(|region| region.source.end <= offset);
        self.carrier
            .get(index)
            .filter(|region| region.source.start <= offset)
    }

    /// EVERY projected region derived from the carrier region containing
    /// `offset`, in ascending projected order. Empty for an elided or
    /// out-of-range offset — a caller that must not act partially reads the
    /// carrier region's class rather than the emptiness of this slice.
    #[must_use]
    pub fn projections_at_carrier(&self, offset: u32) -> Vec<&ProjectedRegion> {
        self.carrier_at(offset)
            .map(|region| {
                region
                    .projected
                    .iter()
                    .filter_map(|index| self.projected.get(*index as usize))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Append `region`, coalescing it into the previous one when the two are one
/// region cut in half by a chunk boundary: same class, contiguous projected
/// bytes, and either both synthesized or contiguous carrier bytes. This is what
/// makes the product COMPACT — a run of untouched source split across many
/// chunks by unrelated edits publishes one region, not one per chunk.
fn push_coalesced(out: &mut Vec<ProjectedRegion>, region: ProjectedRegion) {
    if region.generated.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        let joinable = last.class == region.class
            && last.generated.end == region.generated.start
            && match (last.carrier, region.carrier) {
                (None, None) => true,
                (Some(a), Some(b)) => !a.is_empty() && !b.is_empty() && a.end == b.start,
                _ => false,
            };
        if joinable {
            last.generated.end = region.generated.end;
            if let (Some(a), Some(b)) = (last.carrier.as_mut(), region.carrier) {
                a.end = b.end;
            }
            return;
        }
    }
    out.push(region);
}

fn push_synthesized(out: &mut Vec<ProjectedRegion>, generated: &mut u32, len: u32) {
    push_coalesced(
        out,
        ProjectedRegion {
            generated: Span {
                start: *generated,
                end: *generated + len,
            },
            class: ProjectedClass::Synthesized,
            carrier: None,
        },
    );
    *generated += len;
}

/// Walk the chunks in output order and classify every projected byte.
fn project(transform: &CodeTransform<'_>) -> (Vec<ProjectedRegion>, Vec<InsertionAnchor>) {
    let mut out: Vec<ProjectedRegion> = Vec::with_capacity(transform.chunk_slice().len() + 2);
    let mut insertion_anchors = Vec::with_capacity(1);
    let mut generated: u32 = 0;

    let preamble = transform.helper_preamble_content();
    let carrier_anchor = transform.helper_preamble_carrier_anchor();
    let mut record_anchor = |content: &str, projected: u32| {
        if let (Some(preamble), Some(carrier)) = (preamble, carrier_anchor) {
            if std::ptr::eq(preamble.as_ptr(), content.as_ptr()) && preamble.len() == content.len()
            {
                insertion_anchors.push(InsertionAnchor { projected, carrier });
            }
        }
    };

    record_anchor(transform.intro_text(), generated);
    push_synthesized(
        &mut out,
        &mut generated,
        transform.intro_text().len() as u32,
    );

    // The high-water mark of authored source already emitted. Identity means
    // the region's correspondence is a pure offset delta consistent with
    // everything emitted before it; once carrier bytes that FOLLOW a region
    // have already been emitted, that delta is no longer monotone and the
    // correspondence is region-to-region. The distinction is therefore DERIVED
    // from the walk rather than assumed from the chunk variant: a chunk of
    // untouched source that a move jumped over is Relocated even though nothing
    // rewrote it.
    let mut emitted_through: u32 = 0;

    for chunk in transform.chunk_slice() {
        let content = match chunk {
            Chunk::Original { start, end } => &transform.original()[*start as usize..*end as usize],
            Chunk::Moved { content, .. }
            | Chunk::Overwritten { content, .. }
            | Chunk::InsertedMapped { content, .. }
            | Chunk::OverwrittenSegmented { content, .. }
            | Chunk::Inserted { content }
            | Chunk::InsertedAnchored { content, .. } => content,
        };
        record_anchor(content, generated);
        match chunk {
            Chunk::Original { start, end } => {
                let len = end.saturating_sub(*start);
                if len > 0 {
                    let class = if *start >= emitted_through {
                        ProjectedClass::Identity
                    } else {
                        ProjectedClass::Relocated
                    };
                    push_coalesced(
                        &mut out,
                        ProjectedRegion {
                            generated: Span {
                                start: generated,
                                end: generated + len,
                            },
                            class,
                            carrier: Some(Span {
                                start: *start,
                                end: *end,
                            }),
                        },
                    );
                    emitted_through = emitted_through.max(*end);
                }
                generated += len;
            }
            Chunk::Moved {
                start,
                end,
                content,
                replacement,
            } => {
                let len = content.len() as u32;
                if len > 0 {
                    push_coalesced(
                        &mut out,
                        ProjectedRegion {
                            generated: Span {
                                start: generated,
                                end: generated + len,
                            },
                            class: if *replacement {
                                ProjectedClass::Rewritten
                            } else {
                                ProjectedClass::Relocated
                            },
                            carrier: Some(Span {
                                start: *start,
                                end: *end,
                            }),
                        },
                    );
                    emitted_through = emitted_through.max(*end);
                    generated += len;
                }
            }
            Chunk::Overwritten {
                start,
                end,
                content,
            } => {
                let len = content.len() as u32;
                if len > 0 {
                    if transform.is_unmapped_overwrite(content) {
                        // Wholly synthetic replacement text: it has no
                        // correspondence to the span it stands in for, not even
                        // the single "starts here" claim an ordinary overwrite
                        // carries. The replaced carrier extent is then left with
                        // no projection at all, which is what makes it Elided.
                        push_synthesized(&mut out, &mut generated, len);
                    } else {
                        push_coalesced(
                            &mut out,
                            ProjectedRegion {
                                generated: Span {
                                    start: generated,
                                    end: generated + len,
                                },
                                class: ProjectedClass::Rewritten,
                                carrier: Some(Span {
                                    start: *start,
                                    end: *end,
                                }),
                            },
                        );
                        emitted_through = emitted_through.max(*end);
                        generated += len;
                    }
                }
            }
            Chunk::InsertedMapped {
                content,
                source_start,
                content_offset,
            } => {
                let len = content.len() as u32;
                let offset = (*content_offset).min(len);
                push_synthesized(&mut out, &mut generated, offset);
                if offset < len {
                    push_coalesced(
                        &mut out,
                        ProjectedRegion {
                            generated: Span {
                                start: generated,
                                end: generated + (len - offset),
                            },
                            class: ProjectedClass::Rewritten,
                            // A mapped insertion stands at a carrier POINT: it
                            // accounts for no authored extent, so its preimage
                            // is the zero-width span at that point.
                            carrier: Some(Span {
                                start: *source_start,
                                end: *source_start,
                            }),
                        },
                    );
                    generated += len - offset;
                }
            }
            Chunk::OverwrittenSegmented {
                content, anchors, ..
            } => {
                let content_len = content.len() as u32;
                let mut local: u32 = 0;
                for anchor in *anchors {
                    if anchor.length == 0 || anchor.content_offset < local {
                        continue;
                    }
                    push_synthesized(&mut out, &mut generated, anchor.content_offset - local);
                    push_coalesced(
                        &mut out,
                        ProjectedRegion {
                            generated: Span {
                                start: generated,
                                end: generated + anchor.length,
                            },
                            // An anchor is an authored lexeme copied verbatim
                            // into generated scaffolding that shifted it: exact
                            // bytes at a different position.
                            class: ProjectedClass::Relocated,
                            carrier: Some(Span {
                                start: anchor.source_pos,
                                end: anchor.source_pos + anchor.length,
                            }),
                        },
                    );
                    emitted_through = emitted_through.max(anchor.source_pos + anchor.length);
                    generated += anchor.length;
                    local = anchor.content_offset + anchor.length;
                }
                push_synthesized(&mut out, &mut generated, content_len.saturating_sub(local));
            }
            Chunk::Inserted { content } | Chunk::InsertedAnchored { content, .. } => {
                push_synthesized(&mut out, &mut generated, content.len() as u32);
            }
        }
    }

    push_synthesized(
        &mut out,
        &mut generated,
        transform.outro_text().len() as u32,
    );
    insertion_anchors.sort_unstable();
    insertion_anchors.dedup();
    (out, insertion_anchors)
}

/// The carrier class a projected class contributes to the authored bytes it
/// covers. A carrier byte emitted verbatim somewhere is Identity even if it is
/// ALSO rewritten elsewhere: the strongest correspondence any of its projections
/// carries is the disposition the byte deserves, and the weaker projections stay
/// listed on the region so nothing is lost by that choice.
fn contributed(class: ProjectedClass) -> Option<CarrierClass> {
    match class {
        ProjectedClass::Identity => Some(CarrierClass::Identity),
        ProjectedClass::Relocated => Some(CarrierClass::Relocated),
        ProjectedClass::Rewritten => Some(CarrierClass::Rewritten),
        ProjectedClass::Synthesized => None,
    }
}

/// Partition the carrier surface at every projected preimage boundary, then
/// attach to each atomic interval every projection covering it.
fn partition_carrier(projected: &[ProjectedRegion], carrier_len: u32) -> Vec<CarrierRegion> {
    if carrier_len == 0 {
        return Vec::new();
    }

    // Byte-covering preimages, sorted by start — the sweep's input.
    let mut spans: Vec<(u32, u32, u32)> = Vec::new();
    // Zero-width preimages: carrier POINTS, which cover no byte and so cannot be
    // swept, but must still be answerable from the carrier side.
    let mut points: Vec<(u32, u32)> = Vec::new();
    let mut cuts: Vec<u32> = vec![0, carrier_len];
    for (index, region) in projected.iter().enumerate() {
        let Some(span) = region.carrier else { continue };
        let index = index as u32;
        if span.is_empty() {
            if span.start < carrier_len {
                points.push((span.start, index));
                cuts.push(span.start);
            }
            continue;
        }
        let (start, end) = (span.start.min(carrier_len), span.end.min(carrier_len));
        if start >= end {
            continue;
        }
        spans.push((start, end, index));
        cuts.push(start);
        cuts.push(end);
    }
    spans.sort_unstable();
    points.sort_unstable();
    cuts.sort_unstable();
    cuts.dedup();

    let mut out: Vec<CarrierRegion> = Vec::with_capacity(cuts.len().saturating_sub(1));
    let mut next_span = 0usize;
    let mut active: Vec<(u32, u32)> = Vec::new(); // (preimage end, projected index)
    let mut next_point = 0usize;

    for window in cuts.windows(2) {
        let (start, end) = (window[0], window[1]);
        if start >= end {
            continue;
        }
        active.retain(|(span_end, _)| *span_end > start);
        while next_span < spans.len() && spans[next_span].0 <= start {
            let (_, span_end, index) = spans[next_span];
            if span_end > start {
                active.push((span_end, index));
            }
            next_span += 1;
        }

        let mut class: Option<CarrierClass> = None;
        let mut members: Vec<u32> = Vec::with_capacity(active.len() + 1);
        for (_, index) in &active {
            members.push(*index);
            if let Some(contribution) = contributed(projected[*index as usize].class) {
                class = Some(class.map_or(contribution, |current| current.min(contribution)));
            }
        }
        // A carrier point lands on the region BEGINNING at it. It adds a
        // projection to answer with; it never decides the region's class,
        // because it accounts for none of the region's bytes.
        while next_point < points.len() && points[next_point].0 < start {
            next_point += 1;
        }
        let mut point_cursor = next_point;
        while point_cursor < points.len() && points[point_cursor].0 == start {
            members.push(points[point_cursor].1);
            point_cursor += 1;
        }

        members.sort_unstable();
        members.dedup();
        let class = class.unwrap_or(CarrierClass::Elided);
        if class == CarrierClass::Elided {
            // An elided byte has no correspondence at all. A point that merely
            // begins here is not a projection OF these bytes, so publishing it
            // would be the mapped-to-a-neighbour answer the class refuses.
            members.clear();
        }
        out.push(CarrierRegion {
            source: Span { start, end },
            class,
            projected: members,
        });
    }

    out
}

// The geometry plane answers without an engine. A field carrying a callback, a
// trait object, or a borrowed handle into a semantic session would fail one of
// these bounds, so the prohibition is a compile error rather than a convention.
const _: fn() = || {
    fn assert_plain_owned_geometry<T: 'static + Send + Sync + Clone + Eq + std::fmt::Debug>() {}
    assert_plain_owned_geometry::<MappingProduct>();
    assert_plain_owned_geometry::<ProjectedRegion>();
    assert_plain_owned_geometry::<CarrierRegion>();
    assert_plain_owned_geometry::<InsertionAnchor>();
};
