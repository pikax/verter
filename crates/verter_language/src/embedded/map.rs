//! One map stage: segments from a stage's output back to its input, answered
//! exactly or refused with a reason.

use verter_identity::identity::SourceUnitId;
use verter_span::{
    CoordError, EmbeddedByteOffset, EmbeddedByteRange, GeneratedByteOffset, GeneratedByteRange,
    SourceByteOffset, SourceByteRange,
};

use super::chain::MapBasis;
use super::lineage::BaseUri;

/// The byte coordinate domains a stage endpoint can live in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CoordinateDomain {
    Source,
    Embedded,
    Generated,
}

/// A byte offset in a named domain, so a stage can state its endpoints without
/// losing the type that keeps domains apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainOffset {
    Source(SourceByteOffset),
    Embedded(EmbeddedByteOffset),
    Generated(GeneratedByteOffset),
}

impl DomainOffset {
    pub fn domain(self) -> CoordinateDomain {
        match self {
            DomainOffset::Source(_) => CoordinateDomain::Source,
            DomainOffset::Embedded(_) => CoordinateDomain::Embedded,
            DomainOffset::Generated(_) => CoordinateDomain::Generated,
        }
    }

    fn raw(self) -> u32 {
        match self {
            DomainOffset::Source(offset) => offset.get(),
            DomainOffset::Embedded(offset) => offset.get(),
            DomainOffset::Generated(offset) => offset.get(),
        }
    }

    /// The empty range at this offset.
    fn empty_range(self) -> DomainRange {
        match self {
            DomainOffset::Source(at) => DomainRange::Source(SourceByteRange::new(at, at)),
            DomainOffset::Embedded(at) => DomainRange::Embedded(EmbeddedByteRange::new(at, at)),
            DomainOffset::Generated(at) => DomainRange::Generated(GeneratedByteRange::new(at, at)),
        }
    }
}

/// A half-open byte range in a named domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainRange {
    Source(SourceByteRange),
    Embedded(EmbeddedByteRange),
    Generated(GeneratedByteRange),
}

impl DomainRange {
    pub fn domain(self) -> CoordinateDomain {
        match self {
            DomainRange::Source(_) => CoordinateDomain::Source,
            DomainRange::Embedded(_) => CoordinateDomain::Embedded,
            DomainRange::Generated(_) => CoordinateDomain::Generated,
        }
    }

    fn start(self) -> u32 {
        match self {
            DomainRange::Source(range) => range.start().get(),
            DomainRange::Embedded(range) => range.start().get(),
            DomainRange::Generated(range) => range.start().get(),
        }
    }

    fn end(self) -> u32 {
        match self {
            DomainRange::Source(range) => range.end().get(),
            DomainRange::Embedded(range) => range.end().get(),
            DomainRange::Generated(range) => range.end().get(),
        }
    }

    fn len(self) -> u32 {
        self.end() - self.start()
    }

    /// A range in this domain. Callers pass ordered bounds derived from an
    /// already valid range of the same domain.
    fn with_bounds(self, start: u32, end: u32) -> Self {
        match self {
            DomainRange::Source(_) => DomainRange::Source(SourceByteRange::new(
                SourceByteOffset::new(start),
                SourceByteOffset::new(end),
            )),
            DomainRange::Embedded(_) => DomainRange::Embedded(EmbeddedByteRange::new(
                EmbeddedByteOffset::new(start),
                EmbeddedByteOffset::new(end),
            )),
            DomainRange::Generated(_) => DomainRange::Generated(GeneratedByteRange::new(
                GeneratedByteOffset::new(start),
                GeneratedByteOffset::new(end),
            )),
        }
    }

    fn offset_at(self, offset: u32) -> DomainOffset {
        match self {
            DomainRange::Source(_) => DomainOffset::Source(SourceByteOffset::new(offset)),
            DomainRange::Embedded(_) => DomainOffset::Embedded(EmbeddedByteOffset::new(offset)),
            DomainRange::Generated(_) => DomainOffset::Generated(GeneratedByteOffset::new(offset)),
        }
    }
}

/// The transform a stage records. Kinds never merge: each answers for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StageKind {
    Syntax,
    Preprocessor,
    Compiler,
    RuntimeBuild,
    CodeGeneration,
}

/// Whether a stage may certify an edit. A stage's exact blame is not by itself
/// proof that replacement text can travel back through it, and runtime maps
/// certify provenance only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditCertification {
    Reversible,
    ProvenanceOnly,
}

/// How an output run relates to the input it replaces as a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WholeKind {
    /// An escape cooked to its scalar; re-spelled through reverse encoding.
    Escape,
    /// A terminator normalized to another; a CRLF's elided CR is inside it.
    LineTerminatorContraction,
    /// A rewrite with no inverse: blame only, whatever the stage certifies.
    NonInvertibleExpansion,
}

/// An exact authored location in a named unit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthoredOrigin {
    pub unit: SourceUnitId,
    pub base_uri: BaseUri,
    pub range: SourceByteRange,
}

/// One origin of an output run: a range of this stage's input, or authored
/// bytes of another unit (an import a preprocessor inlined).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OriginRef {
    Input(DomainRange),
    Foreign(AuthoredOrigin),
}

/// Where a segment's output came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SegmentOrigin {
    /// Byte-identical input: interior offsets correspond one to one, and
    /// nothing else may be interpolated.
    Verbatim { input: DomainRange },
    /// The run maps only as a whole; its interior has no authored position.
    Whole { input: DomainRange, kind: WholeKind },
    /// Ordered origins for display, in the producer's declared order. No
    /// unique preimage exists.
    Provenance { origins: Box<[OriginRef]> },
    /// Adapter-generated output, with the hole it stands for if any.
    Synthetic { hole: Option<u32> },
    /// Coincident segments disagreeing about their origin, in declared order.
    /// Neither wins.
    Conflicting { origins: Box<[SegmentOrigin]> },
}

/// One output run of a stage. Empty runs do not exist: removed input is an
/// [`Elision`], not an empty mapped run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapSegment {
    pub output: DomainRange,
    pub origin: SegmentOrigin,
}

/// Why input bytes have no output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElisionKind {
    Delimiter,
    LineContinuation,
    Indentation,
    /// An interpolation hole; its expression is a separate region.
    Hole,
}

/// Input bytes removed at an output offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Elision {
    pub at: DomainOffset,
    pub input: DomainRange,
    pub kind: ElisionKind,
}

/// Why a query has no exact answer. An answer is exact or carries one of these;
/// it is never the nearest token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmappable {
    SyntheticOutput,
    ElidedSource,
    NonInvertibleExpansion,
    Hole,
    /// No unique preimage: conflicting, repeated or discontiguous origins.
    AmbiguousPreimage,
    /// The stage exists but has no trustworthy map, or its map has no segment
    /// for these bytes.
    MissingMap {
        stage: StageKind,
    },
    /// Same unit and domain, other content or map revision.
    StaleBasis {
        stage: StageKind,
    },
    MismatchedBasis {
        stage: StageKind,
    },
    InvalidBoundary(CoordError),
    /// The stage certifies provenance only.
    NotEditable {
        stage: StageKind,
    },
}

/// A stage's segments do not describe a consistent function of its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapStageError {
    /// An endpoint is not in the domain its basis declares.
    DomainMismatch,
    /// A verbatim segment's two sides differ in length.
    VerbatimLengthMismatch,
    /// A run maps empty output; removed input is an [`Elision`].
    EmptyOutputRun,
    /// A segment or elision lies past the declared output extent.
    OutOfExtent,
    /// Two segments overlap without coinciding.
    OverlappingSegments,
}

/// Authored origins of a query, in output order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthoredProvenance {
    origins: Box<[AuthoredOrigin]>,
}

impl AuthoredProvenance {
    pub(super) fn new(origins: Vec<AuthoredOrigin>) -> Self {
        Self {
            origins: origins.into_boxed_slice(),
        }
    }

    pub fn origins(&self) -> &[AuthoredOrigin] {
        &self.origins
    }

    /// The single origin, when there is exactly one.
    pub fn exact(&self) -> Option<&AuthoredOrigin> {
        match &*self.origins {
            [origin] => Some(origin),
            _ => None,
        }
    }
}

/// The segment data a mapped stage is built from, in any discovery order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StageSegments {
    pub segments: Vec<MapSegment>,
    pub elisions: Vec<Elision>,
    /// The input position of an empty output, when explicitly declared. No
    /// other anchor exists.
    pub empty_anchor: Option<DomainOffset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StageMap {
    /// Segments sorted by output start with coincident ones collapsed, and
    /// elisions sorted by output offset in declared order at one offset.
    Mapped(StageSegments),
    /// A transform output without a trustworthy map, represented truthfully.
    Missing,
}

/// One independently identified transform from its input to its output basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapStage {
    kind: StageKind,
    certification: EditCertification,
    input: MapBasis,
    output: MapBasis,
    output_extent: u32,
    map: StageMap,
}

impl MapStage {
    /// Build a stage. Segments sort by output range; coincident ones keep their
    /// declared order, collapsing when they agree and becoming
    /// [`SegmentOrigin::Conflicting`] when they do not.
    pub fn new(
        kind: StageKind,
        certification: EditCertification,
        input: MapBasis,
        output: MapBasis,
        output_extent: u32,
        parts: StageSegments,
    ) -> Result<Self, MapStageError> {
        validate_parts(&input, &output, output_extent, &parts)?;
        let StageSegments {
            segments,
            mut elisions,
            empty_anchor,
        } = parts;
        let segments = canonical_segments(segments)?;
        elisions.sort_by_key(|elision| elision.at.raw());
        Ok(Self {
            kind,
            certification,
            input,
            output,
            output_extent,
            map: StageMap::Mapped(StageSegments {
                segments,
                elisions,
                empty_anchor,
            }),
        })
    }

    /// A stage whose output exists but has no trustworthy map. An absent
    /// optional stage is simply not in the chain; this one is present and says
    /// it cannot answer.
    pub fn missing(kind: StageKind, input: MapBasis, output: MapBasis, output_extent: u32) -> Self {
        Self {
            kind,
            certification: EditCertification::ProvenanceOnly,
            input,
            output,
            output_extent,
            map: StageMap::Missing,
        }
    }

    pub fn kind(&self) -> StageKind {
        self.kind
    }

    pub fn certification(&self) -> EditCertification {
        self.certification
    }

    pub fn input(&self) -> &MapBasis {
        &self.input
    }

    pub fn output(&self) -> &MapBasis {
        &self.output
    }

    pub fn output_extent(&self) -> u32 {
        self.output_extent
    }

    /// Canonically ordered segments; empty for a stage with no map.
    pub fn segments(&self) -> &[MapSegment] {
        match &self.map {
            StageMap::Mapped(parts) => &parts.segments,
            StageMap::Missing => &[],
        }
    }

    /// Elisions by output offset; empty for a stage with no map.
    pub fn elisions(&self) -> &[Elision] {
        match &self.map {
            StageMap::Mapped(parts) => &parts.elisions,
            StageMap::Missing => &[],
        }
    }

    pub(super) fn push_foreign_units(&self, units: &mut Vec<SourceUnitId>) {
        for segment in self.segments() {
            let SegmentOrigin::Provenance { origins } = &segment.origin else {
                continue;
            };
            for origin in origins.iter() {
                if let OriginRef::Foreign(foreign) = origin {
                    if !units.contains(&foreign.unit) {
                        units.push(foreign.unit.clone());
                    }
                }
            }
        }
    }

    fn mapped(&self, domain: CoordinateDomain, end: u32) -> Result<&StageSegments, Unmappable> {
        if domain != self.output.domain {
            return Err(Unmappable::MismatchedBasis { stage: self.kind });
        }
        if end > self.output_extent {
            return Err(Unmappable::InvalidBoundary(CoordError::InvalidBoundary));
        }
        match &self.map {
            StageMap::Mapped(parts) => Ok(parts),
            StageMap::Missing => Err(Unmappable::MissingMap { stage: self.kind }),
        }
    }

    /// The input position of the scalar starting at `offset`, or at the end of
    /// the output the input end of the last scalar.
    pub(super) fn position(&self, offset: DomainOffset) -> Result<DomainOffset, Unmappable> {
        let at = offset.raw();
        let parts = self.mapped(offset.domain(), at)?;
        let segments = &parts.segments;
        let index = segments.partition_point(|segment| segment.output.end() <= at);
        if let Some(segment) = segments.get(index).filter(|s| s.output.start() <= at) {
            return match &segment.origin {
                SegmentOrigin::Verbatim { input } => {
                    Ok(input.offset_at(input.start() + (at - segment.output.start())))
                }
                SegmentOrigin::Whole { input, .. } if at == segment.output.start() => {
                    Ok(input.offset_at(input.start()))
                }
                SegmentOrigin::Whole { .. } => Err(Unmappable::NonInvertibleExpansion),
                SegmentOrigin::Provenance { .. } | SegmentOrigin::Conflicting { .. } => {
                    Err(Unmappable::AmbiguousPreimage)
                }
                SegmentOrigin::Synthetic { .. } => Err(Unmappable::SyntheticOutput),
            };
        }
        if at < self.output_extent {
            return Err(Unmappable::MissingMap { stage: self.kind });
        }
        if let Some(last) = index.checked_sub(1).and_then(|last| segments.get(last)) {
            return match &last.origin {
                SegmentOrigin::Verbatim { input } | SegmentOrigin::Whole { input, .. } => {
                    Ok(input.offset_at(input.end()))
                }
                SegmentOrigin::Provenance { .. } | SegmentOrigin::Conflicting { .. } => {
                    Err(Unmappable::AmbiguousPreimage)
                }
                SegmentOrigin::Synthetic { .. } => Err(Unmappable::SyntheticOutput),
            };
        }
        if let Some(anchor) = parts.empty_anchor {
            return Ok(anchor);
        }
        let barrier = parts
            .elisions
            .iter()
            .find_map(|elision| barrier_reason(elision.kind));
        Err(barrier.unwrap_or(Unmappable::MissingMap { stage: self.kind }))
    }

    /// Append the origins of `range` in output order, joining pieces that are
    /// contiguous in the input.
    pub(super) fn provenance(
        &self,
        range: DomainRange,
        origins: &mut Vec<OriginRef>,
    ) -> Result<(), Unmappable> {
        let parts = self.mapped(range.domain(), range.end())?;
        if range.len() == 0 {
            let position = self.position(range.offset_at(range.start()))?;
            origins.push(OriginRef::Input(position.empty_range()));
            return Ok(());
        }
        let mut cursor = range.start();
        let first = parts
            .segments
            .partition_point(|segment| segment.output.end() <= cursor);
        for segment in &parts.segments[first..] {
            if segment.output.start() >= range.end() {
                break;
            }
            if segment.output.start() > cursor {
                return Err(Unmappable::MissingMap { stage: self.kind });
            }
            let local_end = segment.output.end().min(range.end());
            match &segment.origin {
                SegmentOrigin::Verbatim { input } => {
                    let start = input.start() + (cursor - segment.output.start());
                    let piece = input.with_bounds(start, start + (local_end - cursor));
                    push_joined(origins, piece);
                }
                SegmentOrigin::Whole { input, .. } => {
                    require_whole(segment, cursor, local_end)?;
                    push_joined(origins, *input);
                }
                SegmentOrigin::Provenance { origins: declared } => {
                    require_whole(segment, cursor, local_end)?;
                    origins.extend(declared.iter().cloned());
                }
                SegmentOrigin::Synthetic { .. } => return Err(Unmappable::SyntheticOutput),
                SegmentOrigin::Conflicting { .. } => return Err(Unmappable::AmbiguousPreimage),
            }
            cursor = local_end;
        }
        if cursor < range.end() {
            return Err(Unmappable::MissingMap { stage: self.kind });
        }
        Ok(())
    }

    /// The unique input range an edit of `range` replaces in this stage.
    pub(super) fn editable_preimage(&self, range: DomainRange) -> Result<DomainRange, Unmappable> {
        let parts = self.mapped(range.domain(), range.end())?;
        if self.certification != EditCertification::Reversible {
            return Err(Unmappable::NotEditable { stage: self.kind });
        }
        let (start, end) = (range.start(), range.end());
        let barrier = parts
            .elisions
            .iter()
            .filter(|elision| {
                let at = elision.at.raw();
                if start == end {
                    at == start
                } else {
                    start < at && at < end
                }
            })
            .find_map(|elision| barrier_reason(elision.kind));
        if let Some(reason) = barrier {
            return Err(reason);
        }
        if start == end {
            return Ok(self.position(range.offset_at(start))?.empty_range());
        }
        let first = parts
            .segments
            .partition_point(|segment| segment.output.end() <= start);
        for segment in &parts.segments[first..] {
            if segment.output.start() >= end {
                break;
            }
            match &segment.origin {
                SegmentOrigin::Whole {
                    kind: WholeKind::NonInvertibleExpansion,
                    ..
                } => return Err(Unmappable::NonInvertibleExpansion),
                SegmentOrigin::Provenance { .. } => return Err(Unmappable::AmbiguousPreimage),
                _ => {}
            }
        }
        let mut origins = Vec::new();
        self.provenance(range, &mut origins)?;
        match origins.as_slice() {
            [OriginRef::Input(input)] => Ok(*input),
            _ => Err(Unmappable::AmbiguousPreimage),
        }
    }
}

fn barrier_reason(kind: ElisionKind) -> Option<Unmappable> {
    match kind {
        // Delimiters sit outside the mapped text, so nothing crosses them.
        ElisionKind::Delimiter => None,
        ElisionKind::LineContinuation | ElisionKind::Indentation => Some(Unmappable::ElidedSource),
        ElisionKind::Hole => Some(Unmappable::Hole),
    }
}

fn require_whole(segment: &MapSegment, start: u32, end: u32) -> Result<(), Unmappable> {
    if start == segment.output.start() && end == segment.output.end() {
        Ok(())
    } else {
        Err(Unmappable::NonInvertibleExpansion)
    }
}

fn push_joined(origins: &mut Vec<OriginRef>, piece: DomainRange) {
    if let Some(OriginRef::Input(last)) = origins.last_mut() {
        if last.domain() == piece.domain() && last.end() == piece.start() {
            *last = last.with_bounds(last.start(), piece.end());
            return;
        }
    }
    origins.push(OriginRef::Input(piece));
}

fn validate_parts(
    input: &MapBasis,
    output: &MapBasis,
    output_extent: u32,
    parts: &StageSegments,
) -> Result<(), MapStageError> {
    let check_input = |range: &DomainRange| {
        if range.domain() == input.domain {
            Ok(())
        } else {
            Err(MapStageError::DomainMismatch)
        }
    };
    if parts
        .empty_anchor
        .is_some_and(|anchor| anchor.domain() != input.domain)
    {
        return Err(MapStageError::DomainMismatch);
    }
    for segment in &parts.segments {
        if segment.output.domain() != output.domain {
            return Err(MapStageError::DomainMismatch);
        }
        if segment.output.len() == 0 {
            return Err(MapStageError::EmptyOutputRun);
        }
        if segment.output.end() > output_extent {
            return Err(MapStageError::OutOfExtent);
        }
        validate_origin(&segment.origin, &segment.output, &check_input)?;
    }
    for elision in &parts.elisions {
        if elision.at.domain() != output.domain {
            return Err(MapStageError::DomainMismatch);
        }
        if elision.at.raw() > output_extent {
            return Err(MapStageError::OutOfExtent);
        }
        check_input(&elision.input)?;
    }
    Ok(())
}

fn validate_origin(
    origin: &SegmentOrigin,
    output: &DomainRange,
    check_input: &impl Fn(&DomainRange) -> Result<(), MapStageError>,
) -> Result<(), MapStageError> {
    match origin {
        SegmentOrigin::Verbatim { input } => {
            check_input(input)?;
            if input.len() != output.len() {
                return Err(MapStageError::VerbatimLengthMismatch);
            }
        }
        SegmentOrigin::Whole { input, .. } => check_input(input)?,
        SegmentOrigin::Provenance { origins } => {
            for origin in origins.iter() {
                if let OriginRef::Input(input) = origin {
                    check_input(input)?;
                }
            }
        }
        SegmentOrigin::Synthetic { .. } => {}
        SegmentOrigin::Conflicting { origins } => {
            for origin in origins.iter() {
                validate_origin(origin, output, check_input)?;
            }
        }
    }
    Ok(())
}

fn canonical_segments(mut segments: Vec<MapSegment>) -> Result<Vec<MapSegment>, MapStageError> {
    // Stable: coincident segments keep the producer's declared order.
    segments.sort_by_key(|segment| (segment.output.start(), segment.output.end()));
    let mut canonical: Vec<MapSegment> = Vec::with_capacity(segments.len());
    for segment in segments {
        match canonical.last_mut() {
            Some(last) if last.output == segment.output => {
                merge_coincident(last, segment.origin);
            }
            Some(last) if segment.output.start() < last.output.end() => {
                return Err(MapStageError::OverlappingSegments);
            }
            _ => canonical.push(segment),
        }
    }
    Ok(canonical)
}

fn merge_coincident(last: &mut MapSegment, origin: SegmentOrigin) {
    if last.origin == origin {
        return;
    }
    match &mut last.origin {
        SegmentOrigin::Conflicting { origins } => {
            if !origins.contains(&origin) {
                let mut merged = std::mem::take(origins).into_vec();
                merged.push(origin);
                *origins = merged.into_boxed_slice();
            }
        }
        existing => {
            let first = existing.clone();
            *existing = SegmentOrigin::Conflicting {
                origins: Box::new([first, origin]),
            };
        }
    }
}

/// The packed geometry of a decoded stage, for its map revision.
pub(super) fn pack_stage_geometry(segments: &[MapSegment], elisions: &[Elision]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(segments.len() * 20 + elisions.len() * 16);
    let mut push = |value: u32| packed.extend_from_slice(&value.to_le_bytes());
    for segment in segments {
        push(segment.output.start());
        push(segment.output.end());
        match &segment.origin {
            SegmentOrigin::Verbatim { input } => {
                push(0);
                push(input.start());
                push(input.end());
            }
            SegmentOrigin::Whole { input, kind } => {
                push(1 + *kind as u32);
                push(input.start());
                push(input.end());
            }
            SegmentOrigin::Synthetic { hole } => {
                push(8);
                push(hole.unwrap_or(u32::MAX));
            }
            // A decoded literal mints neither of the remaining origins.
            SegmentOrigin::Provenance { .. } | SegmentOrigin::Conflicting { .. } => push(9),
        }
    }
    for elision in elisions {
        push(elision.at.raw());
        push(elision.input.start());
        push(elision.input.end());
        push(elision.kind as u32);
    }
    packed
}

/// A decoded segment at its chunk's new location.
pub(super) fn shift_segment(
    segment: &MapSegment,
    output_delta: i64,
    input_delta: i64,
) -> Result<MapSegment, CoordError> {
    let origin = match &segment.origin {
        SegmentOrigin::Verbatim { input } => SegmentOrigin::Verbatim {
            input: shift_range(*input, input_delta)?,
        },
        SegmentOrigin::Whole { input, kind } => SegmentOrigin::Whole {
            input: shift_range(*input, input_delta)?,
            kind: *kind,
        },
        other => other.clone(),
    };
    Ok(MapSegment {
        output: shift_range(segment.output, output_delta)?,
        origin,
    })
}

/// A decoded elision at its chunk's new location.
pub(super) fn shift_elision(
    elision: &Elision,
    output_delta: i64,
    input_delta: i64,
) -> Result<Elision, CoordError> {
    let at = shift_range(elision.at.empty_range(), output_delta)?;
    Ok(Elision {
        at: at.offset_at(at.start()),
        input: shift_range(elision.input, input_delta)?,
        kind: elision.kind,
    })
}

fn shift_range(range: DomainRange, delta: i64) -> Result<DomainRange, CoordError> {
    let shifted =
        |value: u32| u32::try_from(i64::from(value) + delta).map_err(|_| CoordError::Overflow);
    Ok(range.with_bounds(shifted(range.start())?, shifted(range.end())?))
}
