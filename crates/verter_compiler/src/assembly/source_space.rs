//! Typed source-space coordinates.
//!
//! Two coordinate spaces never share a representation: one GENERATED
//! fragment's own coordinate space ([`FragmentOffset`]/[`FragmentRange`],
//! wired into [`super::fragment::PlacementSlot::Hole`] — a hole offset
//! cannot compile against an assembled-space one), and the final ASSEMBLED
//! artifact's coordinate space ([`AssembledOffset`], wired into
//! [`super::compose::ComposedOutput`]'s placement report). A function that
//! would need to accept a bare `u32`/`Range<u32>` plus a separate "which
//! space" tag is the shape these wrappers exist to make impossible.
//!
//! An AUTHORED-source coordinate wrapper (`Original*`) does not exist here:
//! no [`super::fragment::Fragment`] field currently carries "this
//! fragment's own bytes came from original offset Y" as a fact, so a
//! wrapper for it would be unused scaffolding claiming an invariant
//! nothing enforces. [`SourceSpaceKind::Original`] stays as the space TAG
//! a fragment declares itself in (a fragment moved verbatim from authored
//! source, no codegen pass in between) — add the coordinate wrapper back
//! when a real producer needs to carry an authored-space offset across
//! this boundary.

use std::ops::Range;

use super::fragment::FragmentId;

/// Independent mapping products, never inferred from an output file suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactMapFamily {
    SourceProjection,
    RuntimeSourceMap,
}

/// One authored anchor in a qualified map. Generated coordinates deliberately
/// use a different type from the absolute authored `Span`. Unmapped generated
/// text has no segment and therefore cannot serialize as source geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMapSegment {
    pub generated: Range<u32>,
    pub source_unit: super::source_unit::SourceUnitId,
    pub source_span: verter_span::Span,
}

/// Byte-coordinate mapping data, qualified by an artifact destination and a
/// nonempty set of authored input spaces. No raw or implicit-identity map is
/// accepted. Protocol/V3 adapters own conversion to UTF-16 line/column geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedArtifactMap {
    pub family: ArtifactMapFamily,
    pub generated: super::fragment::ArtifactId,
    /// Exact destination bytes, separate from the artifact's stable lineage.
    pub generated_content: super::source_unit::ContentId,
    /// The same observed inputs that the mapped artifact was produced from.
    pub input_basis: verter_identity::identity::InputBasisId,
    pub sources: std::collections::BTreeSet<super::source_unit::SourceUnitId>,
    pub segments: Vec<ArtifactMapSegment>,
}

impl QualifiedArtifactMap {
    pub(crate) fn validate(
        &mut self,
        artifact: &super::fragment::ArtifactId,
        content: &super::fragment::ArtifactContent,
        provenance: &super::fragment::ArtifactProvenance,
        generated_content: Option<&super::source_unit::ContentId>,
        sources: &std::collections::BTreeMap<
            super::source_unit::SourceUnitId,
            super::source_unit::ArtifactSourceUnit,
        >,
    ) -> Result<(), super::publish::ArtifactSchemaError> {
        use super::publish::ArtifactSchemaError as E;
        if &self.generated != artifact {
            return Err(E::WrongGeneratedSpace);
        }
        if self.sources.is_empty() {
            return Err(E::UnqualifiedMap);
        }
        if !self.sources.is_subset(&provenance.inputs) {
            return Err(E::MapSourceOutsideProvenance);
        }
        if self.input_basis != provenance.input_basis {
            return Err(E::StaleMapInputBasis);
        }
        let super::fragment::ArtifactContent::Available(code) = content else {
            return Err(E::UnavailableMappedContent);
        };
        if Some(&self.generated_content) != generated_content {
            return Err(E::StaleGeneratedContent);
        }
        self.segments.sort_by(|a, b| {
            (a.generated.start, a.generated.end).cmp(&(b.generated.start, b.generated.end))
        });
        let mut previous: Option<&Range<u32>> = None;
        for segment in &self.segments {
            if !self.sources.contains(&segment.source_unit) {
                return Err(E::UnqualifiedMap);
            }
            let source = sources
                .get(&segment.source_unit)
                .ok_or(E::UnknownSourceUnit)?;
            let span = segment.source_span;
            if span.start > span.end
                || span.start < source.source_span.start
                || span.end > source.source_span.end
            {
                return Err(E::InvalidSourceRange);
            }
            let range = &segment.generated;
            if code.get(range.start as usize..range.end as usize).is_none() {
                return Err(E::InvalidGeneratedRange);
            }
            if previous.is_some_and(|p| p.end > range.start || p.start == range.start) {
                return Err(E::OverlappingMappings);
            }
            previous = Some(range);
        }
        Ok(())
    }
}

/// Which space a coordinate lives in — carried for diagnostics only; the
/// typed wrappers below are the actual guard, not this tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSpaceKind {
    Original,
    GeneratedFragment,
    AssembledOutput,
}

/// A byte offset into one GENERATED fragment's own coordinate space —
/// meaningless without knowing which fragment it came from, so the
/// fragment identity travels with the offset rather than being implied by
/// call-site context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentOffset {
    pub fragment: FragmentId,
    pub offset: u32,
}

/// A byte range into one GENERATED fragment's own coordinate space. Used
/// by [`super::fragment::PlacementSlot::Hole`] — a hole is always a range
/// inside its owner's own generated bytes, never an assembled-space one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentRange {
    pub fragment: FragmentId,
    pub range: Range<u32>,
}

/// A byte offset into the final ASSEMBLED artifact — where one
/// contributing fragment's bytes begin in the composed output. Used by
/// [`super::compose::ComposedOutput::fragment_starts_at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssembledOffset(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof, not a runtime assertion: a [`FragmentOffset`]
    /// and an [`AssembledOffset`] are distinct types, so the only way this
    /// function compiles is if the caller passed the matching space.
    fn accepts_only_fragment_offset(offset: FragmentOffset) -> u32 {
        offset.offset
    }

    #[test]
    fn fragment_and_assembled_offsets_are_distinct_types() {
        let fragment_offset = FragmentOffset {
            fragment: FragmentId(0),
            offset: 5,
        };
        assert_eq!(accepts_only_fragment_offset(fragment_offset), 5);
        // `accepts_only_fragment_offset(AssembledOffset(5))` — left
        // uncommented, this is a compile error, which is the invariant
        // this type split exists to enforce.
        let assembled = AssembledOffset(5);
        assert_eq!(assembled.0, 5);
    }
}
