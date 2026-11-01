//! Decoding a literal's authored bytes under a profile's policy.
//!
//! A decode either completes or returns a typed [`CodecRefusal`]. No partial
//! text, map or offset exists on the refusal path, so nothing degraded can be
//! parsed or retained as complete.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::ContentId;
use verter_identity::mapping::MapRevision;
use verter_identity::successor::RegionId;
use verter_span::{
    CoordError, EmbeddedByteOffset, EmbeddedByteRange, SourceByteOffset, SourceByteRange,
};

use super::chain::MapBasis;
use super::cooked::{CookedText, MissingCookedValue, NonUnicodeCookedLiteral, SurrogateKind};
use super::lineage::RegionLineage;
use super::map::{
    pack_stage_geometry, shift_elision, shift_segment, CoordinateDomain, DomainOffset, DomainRange,
    EditCertification, Elision, ElisionKind, MapSegment, MapStage, MapStageError, OriginRef,
    SegmentOrigin, StageKind, StageSegments, Unmappable, WholeKind,
};
use super::policy::{
    CookedPolicy, EmbeddedTextCodec, EscapePolicy, HolePolicy, IdentityEscapes, IndentationPolicy,
    LineTerminatorPolicy, NumericEscape, ProfileHoleClass, ReverseEncoding, TextSelection,
};

/// One interpolation hole as the host parser found it. `outer` includes the
/// hole's delimiters; `expression` is the authored expression inside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoleGeometry {
    pub outer: SourceByteRange,
    pub expression: SourceByteRange,
    pub region: RegionId,
}

/// The parser-minted geometry of one literal. The codec never searches source
/// text for it: only the parser knows where a hole's expression ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralGeometry {
    outer: SourceByteRange,
    body: SourceByteRange,
    holes: Box<[HoleGeometry]>,
}

/// Geometry that cannot describe a literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryError {
    BodyOutsideLiteral,
    HoleOutsideBody { ordinal: u32 },
    HoleOutOfOrder { ordinal: u32 },
    ExpressionOutsideHole { ordinal: u32 },
}

impl LiteralGeometry {
    /// `outer` includes the delimiters, `body` excludes them, and `holes` are
    /// ordered and disjoint inside the body.
    pub fn new(
        outer: SourceByteRange,
        body: SourceByteRange,
        holes: Vec<HoleGeometry>,
    ) -> Result<Self, GeometryError> {
        if body.start() < outer.start() || body.end() > outer.end() {
            return Err(GeometryError::BodyOutsideLiteral);
        }
        let mut cursor = body.start();
        for (ordinal, hole) in (0u32..).zip(&holes) {
            if hole.outer.start() < body.start() || hole.outer.end() > body.end() {
                return Err(GeometryError::HoleOutsideBody { ordinal });
            }
            if hole.outer.start() < cursor {
                return Err(GeometryError::HoleOutOfOrder { ordinal });
            }
            if hole.expression.start() < hole.outer.start()
                || hole.expression.end() > hole.outer.end()
            {
                return Err(GeometryError::ExpressionOutsideHole { ordinal });
            }
            cursor = hole.outer.end();
        }
        Ok(Self {
            outer,
            body,
            holes: holes.into_boxed_slice(),
        })
    }

    pub fn outer(&self) -> SourceByteRange {
        self.outer
    }

    pub fn body(&self) -> SourceByteRange {
        self.body
    }

    pub fn holes(&self) -> &[HoleGeometry] {
        &self.holes
    }

    fn chunk_count(&self) -> usize {
        self.holes.len() + 1
    }

    /// The text run before hole `index`, or after the last hole. Empty runs
    /// exist and count: they keep holes in order.
    fn chunk(&self, index: usize) -> SourceByteRange {
        let start = match index.checked_sub(1) {
            Some(previous) => self.holes[previous].outer.end(),
            None => self.body.start(),
        };
        let end = self
            .holes
            .get(index)
            .map_or(self.body.end(), |hole| hole.outer.start());
        SourceByteRange::new(start, end)
    }
}

/// Everything one decode observes.
#[derive(Debug, Clone, Copy)]
pub struct DecodeRequest<'a> {
    /// The whole authored file the geometry indexes.
    pub source: &'a str,
    pub geometry: &'a LiteralGeometry,
    pub lineage: &'a RegionLineage,
    /// The authored basis of `source`.
    pub input: &'a MapBasis,
}

/// A textual edit from the basis a previous decode observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralEdit {
    pub base: MapBasis,
    pub range: SourceByteRange,
    pub inserted_len: u32,
}

/// Polled between chunks; a true answer abandons the decode.
pub trait DecodeInterrupt {
    fn is_interrupted(&self) -> bool;
}

/// For a caller with nothing to cancel.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverInterrupted;

impl DecodeInterrupt for NeverInterrupted {
    fn is_interrupted(&self) -> bool {
        false
    }
}

/// Why a decode produced no text. Refusals keep source order: the first failing
/// chunk or hole decides, for a fresh decode and a splice alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecRefusal {
    NonUnicodeCookedLiteral(NonUnicodeCookedLiteral),
    MissingCookedValue(MissingCookedValue),
    ForbiddenLineTerminator {
        authored: SourceByteRange,
    },
    /// An unescaped closer inside the body the parser reported.
    UnescapedDelimiter {
        authored: SourceByteRange,
    },
    /// An unescaped hole opener the parser reported no hole for.
    UndeclaredHole {
        authored: SourceByteRange,
    },
    /// The bytes around the body are not the declared delimiters.
    DelimiterMismatch {
        authored: SourceByteRange,
    },
    /// The profile does not admit this hole; `disposition` is its reason.
    HoleRefused {
        ordinal: u32,
        authored: SourceByteRange,
        disposition: Option<ProfileHoleClass>,
    },
    InvalidGeometry(CoordError),
    /// A stale or mismatched basis: the input is not what was observed.
    BasisRefused(Unmappable),
    /// The decoder's own segments were refused. Reaching this is a codec bug,
    /// reported rather than published.
    InconsistentMap(MapStageError),
    Interrupted,
}

/// Deterministic work one decode performed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeWork {
    pub chunks_decoded: u32,
    pub chunks_reused: u32,
    pub bytes_scanned: u64,
    /// Segments in the result, reused chunks included.
    pub segments_emitted: u64,
}

/// One text run of a decoded literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedChunk {
    authored: SourceByteRange,
    output: EmbeddedByteRange,
    segments: (u32, u32),
    elisions: (u32, u32),
}

impl DecodedChunk {
    pub fn authored(&self) -> SourceByteRange {
        self.authored
    }

    pub fn output(&self) -> EmbeddedByteRange {
        self.output
    }
}

/// One admitted hole, in authored order. `output` is its synthetic placeholder
/// bytes, or empty where the hole sits between two chunks' text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoleRegion {
    pub ordinal: u32,
    pub outer: SourceByteRange,
    pub expression: SourceByteRange,
    pub region: RegionId,
    pub class: ProfileHoleClass,
    pub output: EmbeddedByteRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DecodedText {
    Raw(String),
    Cooked(CookedText),
}

/// A complete decode: text, its syntax map, ordered chunks and holes.
///
/// Equality covers every observable answer and ignores [`Self::work`], so a
/// splice can be compared with a fresh decode directly.
#[derive(Debug, Clone)]
pub struct DecodedLiteral {
    text: DecodedText,
    lineage: RegionLineage,
    chunks: Box<[DecodedChunk]>,
    holes: Box<[HoleRegion]>,
    stage: Arc<MapStage>,
    policy: CanonicalDigest,
    work: DecodeWork,
}

impl PartialEq for DecodedLiteral {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.lineage == other.lineage
            && self.chunks == other.chunks
            && self.holes == other.holes
            && self.stage == other.stage
            && self.policy == other.policy
    }
}

impl Eq for DecodedLiteral {}

impl DecodedLiteral {
    /// The decoded UTF-8 text, the only thing an embedded parser may read.
    pub fn text(&self) -> &str {
        match &self.text {
            DecodedText::Raw(text) => text,
            DecodedText::Cooked(text) => text.as_str(),
        }
    }

    /// The validated cooked value; `None` under raw selection, which has no
    /// cooked identity.
    pub fn cooked(&self) -> Option<&CookedText> {
        match &self.text {
            DecodedText::Raw(_) => None,
            DecodedText::Cooked(text) => Some(text),
        }
    }

    pub fn lineage(&self) -> &RegionLineage {
        &self.lineage
    }

    pub fn chunks(&self) -> &[DecodedChunk] {
        &self.chunks
    }

    pub fn holes(&self) -> &[HoleRegion] {
        &self.holes
    }

    /// The syntax stage, ready to start or join an [`super::AuthoredMapChain`].
    pub fn stage(&self) -> &Arc<MapStage> {
        &self.stage
    }

    pub fn work(&self) -> DecodeWork {
        self.work
    }

    /// The authored position of the scalar starting at `offset`.
    pub fn map_position(&self, offset: EmbeddedByteOffset) -> Result<SourceByteOffset, Unmappable> {
        offset
            .validate(self.text())
            .map_err(Unmappable::InvalidBoundary)?;
        match self.stage.position(DomainOffset::Embedded(offset))? {
            DomainOffset::Source(authored) => Ok(authored),
            _ => Err(Unmappable::MismatchedBasis {
                stage: StageKind::Syntax,
            }),
        }
    }

    /// The authored origins of `range`, in output order. More than one means
    /// bytes were removed between them, so they are provenance, not a span.
    pub fn map_range(&self, range: EmbeddedByteRange) -> Result<Vec<SourceByteRange>, Unmappable> {
        range
            .validate(self.text())
            .map_err(Unmappable::InvalidBoundary)?;
        let mut origins = Vec::new();
        self.stage
            .provenance(DomainRange::Embedded(range), &mut origins)?;
        origins
            .into_iter()
            .map(|origin| match origin {
                OriginRef::Input(DomainRange::Source(authored)) => Ok(authored),
                _ => Err(Unmappable::MismatchedBasis {
                    stage: StageKind::Syntax,
                }),
            })
            .collect()
    }

    /// The unique authored range an edit of `range` may replace.
    pub fn editable_range(&self, range: EmbeddedByteRange) -> Result<SourceByteRange, Unmappable> {
        range
            .validate(self.text())
            .map_err(Unmappable::InvalidBoundary)?;
        match self.stage.editable_preimage(DomainRange::Embedded(range))? {
            DomainRange::Source(authored) => Ok(authored),
            _ => Err(Unmappable::MismatchedBasis {
                stage: StageKind::Syntax,
            }),
        }
    }
}

impl EmbeddedTextCodec {
    /// Decode a literal from scratch. Only an explicit call does any work.
    pub fn decode(
        &self,
        request: &DecodeRequest<'_>,
        interrupt: &dyn DecodeInterrupt,
    ) -> Result<DecodedLiteral, CodecRefusal> {
        self.assemble(request, None, interrupt)
    }

    /// Decode after `edit`, re-decoding only chunks the edit reached and
    /// reusing the rest of `previous`. The answer equals a fresh decode of the
    /// edited source, including which refusal comes first.
    ///
    /// `edit` must start from the basis `previous` observed; any other is a
    /// stale edit and is refused. A refused splice leaves `previous` untouched,
    /// still answering only for its own basis.
    pub fn decode_splice(
        &self,
        previous: &DecodedLiteral,
        edit: &LiteralEdit,
        request: &DecodeRequest<'_>,
        interrupt: &dyn DecodeInterrupt,
    ) -> Result<DecodedLiteral, CodecRefusal> {
        previous
            .stage
            .input()
            .require(&edit.base, StageKind::Syntax)
            .map_err(CodecRefusal::BasisRefused)?;
        let reuse = self.reuse_plan(previous, edit, request.geometry);
        self.assemble(request, reuse.as_ref(), interrupt)
    }

    /// Which chunks survive `edit` untouched. Removal of common indentation
    /// depends on every line, so that policy reuses nothing.
    fn reuse_plan<'p>(
        &self,
        previous: &'p DecodedLiteral,
        edit: &LiteralEdit,
        geometry: &LiteralGeometry,
    ) -> Option<ReusePlan<'p>> {
        if previous.policy != self.policy_digest() {
            return None;
        }
        if let TextSelection::Cooked(policy) = self.selection() {
            if policy.indentation != IndentationPolicy::Preserve {
                return None;
            }
        }
        let delta = i64::from(edit.inserted_len) - i64::from(edit.range.len());
        let old = &previous.chunks;
        let new_count = geometry.chunk_count();
        let limit = old.len().min(new_count);
        let front = (0..limit)
            .take_while(|&index| {
                let chunk = old[index].authored;
                chunk == geometry.chunk(index) && chunk.end() <= edit.range.start()
            })
            .count();
        let back = (0..limit - front)
            .take_while(|&from_end| {
                let chunk = old[old.len() - 1 - from_end].authored;
                let new = geometry.chunk(new_count - 1 - from_end);
                chunk.start() >= edit.range.end()
                    && i64::from(new.start().get()) == i64::from(chunk.start().get()) + delta
                    && i64::from(new.end().get()) == i64::from(chunk.end().get()) + delta
            })
            .count();
        Some(ReusePlan {
            previous,
            front,
            back,
            new_count,
        })
    }

    fn assemble(
        &self,
        request: &DecodeRequest<'_>,
        reuse: Option<&ReusePlan<'_>>,
        interrupt: &dyn DecodeInterrupt,
    ) -> Result<DecodedLiteral, CodecRefusal> {
        self.check_request(request)?;
        let geometry = request.geometry;
        let mut assembly = Assembly::new(request.source, geometry.body().len() as usize);
        let (open, close) = delimiter_ranges(geometry);
        if !open.is_empty() {
            assembly.push_elision(open, ElisionKind::Delimiter)?;
        }
        let scan = self.scan_settings(request);
        let mut chunks = Vec::with_capacity(geometry.chunk_count());
        let mut holes = Vec::with_capacity(geometry.holes().len());
        for index in 0..geometry.chunk_count() {
            if interrupt.is_interrupted() {
                return Err(CodecRefusal::Interrupted);
            }
            let authored = geometry.chunk(index);
            let reusable = reuse.and_then(|plan| Some((plan.previous, plan.reusable(index)?)));
            let chunk = match reusable {
                Some((previous, old)) => assembly.reuse_chunk(previous, old, authored)?,
                None => assembly.decode_chunk(self, &scan, authored, index == 0)?,
            };
            chunks.push(chunk);
            if let Some(hole) = geometry.holes().get(index) {
                holes.push(assembly.push_hole(self.holes(), hole, index as u32)?);
            }
        }
        if !close.is_empty() {
            assembly.push_elision(close, ElisionKind::Delimiter)?;
        }
        self.finish(request, assembly, chunks, holes)
    }

    fn check_request(&self, request: &DecodeRequest<'_>) -> Result<(), CodecRefusal> {
        let source = request.source;
        if u32::try_from(source.len()).is_err() {
            return Err(CodecRefusal::InvalidGeometry(CoordError::Overflow));
        }
        if request.input.domain != CoordinateDomain::Source
            || request.input.unit != *request.lineage.unit()
        {
            return Err(CodecRefusal::BasisRefused(Unmappable::MismatchedBasis {
                stage: StageKind::Syntax,
            }));
        }
        let geometry = request.geometry;
        let invalid = CodecRefusal::InvalidGeometry;
        geometry.outer().validate(source).map_err(invalid)?;
        geometry.body().validate(source).map_err(invalid)?;
        for hole in geometry.holes() {
            hole.outer.validate(source).map_err(invalid)?;
            hole.expression.validate(source).map_err(invalid)?;
        }
        let (open, close) = delimiter_ranges(geometry);
        let delimiters = self.delimiters();
        if slice(source, open) != &*delimiters.open {
            return Err(CodecRefusal::DelimiterMismatch { authored: open });
        }
        if slice(source, close) != &*delimiters.close {
            return Err(CodecRefusal::DelimiterMismatch { authored: close });
        }
        Ok(())
    }

    fn scan_settings<'r>(&'r self, request: &DecodeRequest<'r>) -> ScanSettings<'r> {
        let indent = match self.selection() {
            TextSelection::Cooked(CookedPolicy {
                indentation:
                    IndentationPolicy::StripCommonLeading {
                        whitespace,
                        include_first_line,
                    },
                ..
            }) => common_indent(
                request.source,
                request.geometry,
                whitespace,
                *include_first_line,
                self.delimiters().escape_introducer,
            ),
            _ => "",
        };
        let strips_first_line = matches!(
            self.selection(),
            TextSelection::Cooked(CookedPolicy {
                indentation: IndentationPolicy::StripCommonLeading {
                    include_first_line: true,
                    ..
                },
                ..
            })
        );
        ScanSettings {
            introducer: self.delimiters().escape_introducer,
            close: &self.delimiters().close,
            opener: self.holes().opener().unwrap_or(""),
            indent,
            strips_first_line,
        }
    }

    fn finish(
        &self,
        request: &DecodeRequest<'_>,
        assembly: Assembly<'_>,
        chunks: Vec<DecodedChunk>,
        holes: Vec<HoleRegion>,
    ) -> Result<DecodedLiteral, CodecRefusal> {
        let Assembly {
            text,
            segments,
            elisions,
            mut work,
            ..
        } = assembly;
        work.segments_emitted = segments.len() as u64;
        let output_extent = offset_of(text.len())?.get();
        let body = request.geometry.body();
        let empty_anchor =
            (body.is_empty() && text.is_empty()).then_some(DomainOffset::Source(body.start()));
        let content = ContentId::from_content_bytes(text.as_bytes());
        let map_revision = MapRevision::from_canonical(&DecodedMapDescriptor {
            policy: self.policy_digest(),
            input_content: request.input.content.digest(),
            geometry: CanonicalDigest::of_bytes(&pack_stage_geometry(&segments, &elisions)),
        });
        let output = MapBasis {
            unit: request.input.unit.clone(),
            domain: CoordinateDomain::Embedded,
            content,
            map_revision,
        };
        let certification = match self.selection() {
            TextSelection::Cooked(policy) if policy.reverse == ReverseEncoding::Undeclared => {
                EditCertification::ProvenanceOnly
            }
            _ => EditCertification::Reversible,
        };
        let stage = MapStage::new(
            StageKind::Syntax,
            certification,
            request.input.clone(),
            output,
            output_extent,
            StageSegments {
                segments,
                elisions,
                empty_anchor,
            },
        )
        .map_err(CodecRefusal::InconsistentMap)?;
        let text = match self.selection() {
            TextSelection::Raw => DecodedText::Raw(text),
            TextSelection::Cooked(_) => DecodedText::Cooked(CookedText::from_validated(text)),
        };
        Ok(DecodedLiteral {
            text,
            lineage: request.lineage.clone(),
            chunks: chunks.into_boxed_slice(),
            holes: holes.into_boxed_slice(),
            stage: Arc::new(stage),
            policy: self.policy_digest(),
            work,
        })
    }
}

struct DecodedMapDescriptor {
    policy: CanonicalDigest,
    input_content: CanonicalDigest,
    geometry: CanonicalDigest,
}

impl CanonicalEncode for DecodedMapDescriptor {
    const DOMAIN_TAG: &'static str = "verter.language.embedded.decoded_map.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, self.policy.as_bytes());
        encoder.field_bytes(2, self.input_content.as_bytes());
        encoder.field_bytes(3, self.geometry.as_bytes());
    }
}

struct ReusePlan<'p> {
    previous: &'p DecodedLiteral,
    front: usize,
    back: usize,
    new_count: usize,
}

impl<'p> ReusePlan<'p> {
    fn reusable(&self, index: usize) -> Option<&'p DecodedChunk> {
        let old = &self.previous.chunks;
        if index < self.front {
            return Some(&old[index]);
        }
        let from_end = self.new_count - 1 - index;
        (from_end < self.back).then(|| &old[old.len() - 1 - from_end])
    }
}

/// Fixed per-decode scan inputs.
struct ScanSettings<'r> {
    introducer: Option<char>,
    close: &'r str,
    opener: &'r str,
    indent: &'r str,
    strips_first_line: bool,
}

/// The buffers one decode writes. A refusal drops them all.
struct Assembly<'s> {
    source: &'s str,
    text: String,
    segments: Vec<MapSegment>,
    elisions: Vec<Elision>,
    work: DecodeWork,
}

impl<'s> Assembly<'s> {
    fn new(source: &'s str, capacity: usize) -> Self {
        Self {
            source,
            text: String::with_capacity(capacity),
            segments: Vec::new(),
            elisions: Vec::new(),
            work: DecodeWork::default(),
        }
    }

    fn output_offset(&self) -> Result<EmbeddedByteOffset, CodecRefusal> {
        offset_of(self.text.len())
    }

    fn push_verbatim(&mut self, start: usize, end: usize) -> Result<(), CodecRefusal> {
        if start == end {
            return Ok(());
        }
        let output_start = self.output_offset()?;
        self.text.push_str(&self.source[start..end]);
        let output_end = self.output_offset()?;
        let input = DomainRange::Source(authored_range(start, end));
        let output = DomainRange::Embedded(EmbeddedByteRange::new(output_start, output_end));
        if let Some(MapSegment {
            output: DomainRange::Embedded(last_output),
            origin:
                SegmentOrigin::Verbatim {
                    input: DomainRange::Source(last_input),
                },
        }) = self.segments.last_mut()
        {
            if last_output.end() == output_start && last_input.end().get() as usize == start {
                *last_output = EmbeddedByteRange::new(last_output.start(), output_end);
                *last_input = SourceByteRange::new(last_input.start(), byte_offset(end));
                return Ok(());
            }
        }
        self.segments.push(MapSegment {
            output,
            origin: SegmentOrigin::Verbatim { input },
        });
        Ok(())
    }

    fn push_whole(
        &mut self,
        cooked: char,
        start: usize,
        end: usize,
        kind: WholeKind,
    ) -> Result<(), CodecRefusal> {
        let output_start = self.output_offset()?;
        self.text.push(cooked);
        let output_end = self.output_offset()?;
        self.segments.push(MapSegment {
            output: DomainRange::Embedded(EmbeddedByteRange::new(output_start, output_end)),
            origin: SegmentOrigin::Whole {
                input: DomainRange::Source(authored_range(start, end)),
                kind,
            },
        });
        Ok(())
    }

    fn push_elision(
        &mut self,
        input: SourceByteRange,
        kind: ElisionKind,
    ) -> Result<(), CodecRefusal> {
        let at = DomainOffset::Embedded(self.output_offset()?);
        self.elisions.push(Elision {
            at,
            input: DomainRange::Source(input),
            kind,
        });
        Ok(())
    }

    fn push_hole(
        &mut self,
        policy: &HolePolicy,
        hole: &HoleGeometry,
        ordinal: u32,
    ) -> Result<HoleRegion, CodecRefusal> {
        let (class, placeholder) = match policy {
            HolePolicy::Admit {
                class, placeholder, ..
            } => (class, placeholder),
            HolePolicy::Absent => {
                return Err(CodecRefusal::HoleRefused {
                    ordinal,
                    authored: hole.outer,
                    disposition: None,
                })
            }
            HolePolicy::Refuse { disposition, .. } => {
                return Err(CodecRefusal::HoleRefused {
                    ordinal,
                    authored: hole.outer,
                    disposition: Some(disposition.clone()),
                })
            }
        };
        let start = self.output_offset()?;
        if let Some(placeholder) = placeholder {
            self.text.push_str(&placeholder.prefix);
            self.text.push_str(&ordinal.to_string());
            self.text.push_str(&placeholder.suffix);
        }
        let end = self.output_offset()?;
        let output = EmbeddedByteRange::new(start, end);
        if !output.is_empty() {
            self.segments.push(MapSegment {
                output: DomainRange::Embedded(output),
                origin: SegmentOrigin::Synthetic {
                    hole: Some(ordinal),
                },
            });
        }
        self.push_elision(hole.outer, ElisionKind::Hole)?;
        Ok(HoleRegion {
            ordinal,
            outer: hole.outer,
            expression: hole.expression,
            region: hole.region.clone(),
            class: class.clone(),
            output,
        })
    }

    /// Copy a surviving chunk's output and shift its map to the new location.
    fn reuse_chunk(
        &mut self,
        previous: &DecodedLiteral,
        old: &DecodedChunk,
        authored: SourceByteRange,
    ) -> Result<DecodedChunk, CodecRefusal> {
        let (old_start, old_end) = (old.output.start().get(), old.output.end().get());
        let output_start = self.output_offset()?;
        let output_delta = i64::from(output_start.get()) - i64::from(old_start);
        let input_delta = i64::from(authored.start().get()) - i64::from(old.authored.start().get());
        let (segments_start, elisions_start) = (self.segments.len(), self.elisions.len());
        self.text
            .push_str(&previous.text()[old_start as usize..old_end as usize]);
        let invalid = CodecRefusal::InvalidGeometry;
        let old_segments =
            &previous.stage.segments()[old.segments.0 as usize..old.segments.1 as usize];
        for segment in old_segments {
            self.segments
                .push(shift_segment(segment, output_delta, input_delta).map_err(invalid)?);
        }
        let old_elisions =
            &previous.stage.elisions()[old.elisions.0 as usize..old.elisions.1 as usize];
        for elision in old_elisions {
            self.elisions
                .push(shift_elision(elision, output_delta, input_delta).map_err(invalid)?);
        }
        self.work.chunks_reused += 1;
        Ok(DecodedChunk {
            authored,
            output: EmbeddedByteRange::new(output_start, self.output_offset()?),
            segments: (segments_start as u32, self.segments.len() as u32),
            elisions: (elisions_start as u32, self.elisions.len() as u32),
        })
    }

    fn decode_chunk(
        &mut self,
        codec: &EmbeddedTextCodec,
        scan: &ScanSettings<'_>,
        authored: SourceByteRange,
        first_chunk: bool,
    ) -> Result<DecodedChunk, CodecRefusal> {
        let output_start = self.output_offset()?;
        let (segments_start, elisions_start) = (self.segments.len(), self.elisions.len());
        let (start, end) = (
            authored.start().get() as usize,
            authored.end().get() as usize,
        );
        match codec.selection() {
            TextSelection::Raw => {
                self.scan_raw(scan, start, end)?;
                self.push_verbatim(start, end)?;
            }
            TextSelection::Cooked(policy) => {
                self.decode_cooked(policy, scan, start, end, first_chunk)?;
            }
        }
        self.work.chunks_decoded += 1;
        self.work.bytes_scanned += (end - start) as u64;
        Ok(DecodedChunk {
            authored,
            output: EmbeddedByteRange::new(output_start, self.output_offset()?),
            segments: (segments_start as u32, self.segments.len() as u32),
            elisions: (elisions_start as u32, self.elisions.len() as u32),
        })
    }

    /// Raw text is copied as is; scanning only proves the parser's body has no
    /// unescaped closer or hole opener.
    fn scan_raw(
        &self,
        scan: &ScanSettings<'_>,
        start: usize,
        end: usize,
    ) -> Result<(), CodecRefusal> {
        let mut chars = self.source[start..end].char_indices();
        while let Some((relative, ch)) = chars.next() {
            if scan.introducer == Some(ch) {
                chars.next();
                continue;
            }
            check_syntax_at(self.source, scan, start + relative, end)?;
        }
        Ok(())
    }

    fn decode_cooked(
        &mut self,
        policy: &CookedPolicy,
        scan: &ScanSettings<'_>,
        start: usize,
        end: usize,
        first_chunk: bool,
    ) -> Result<(), CodecRefusal> {
        let source = self.source;
        let mut pos = start;
        if first_chunk && scan.strips_first_line {
            pos = self.strip_indent(scan.indent, pos, end)?;
        }
        let mut run_start = pos;
        while let Some(ch) = source[pos..end].chars().next() {
            if scan.introducer == Some(ch) {
                self.push_verbatim(run_start, pos)?;
                pos = self.decode_escape(&policy.escapes, ch, pos, end)?;
                run_start = pos;
                continue;
            }
            if ch != '\n' && ch != '\r' {
                check_syntax_at(source, scan, pos, end)?;
                pos += ch.len_utf8();
                continue;
            }
            let terminator_end = pos + terminator_len(&source[pos..end]);
            match policy.line_terminators {
                LineTerminatorPolicy::Forbidden => {
                    return Err(CodecRefusal::ForbiddenLineTerminator {
                        authored: authored_range(pos, terminator_end),
                    });
                }
                LineTerminatorPolicy::NormalizeToLf if ch == '\r' => {
                    self.push_verbatim(run_start, pos)?;
                    let kind = WholeKind::LineTerminatorContraction;
                    self.push_whole('\n', pos, terminator_end, kind)?;
                    run_start = terminator_end;
                }
                LineTerminatorPolicy::NormalizeToLf | LineTerminatorPolicy::Preserve => {}
            }
            pos = terminator_end;
            if !scan.indent.is_empty() {
                self.push_verbatim(run_start, pos)?;
                pos = self.strip_indent(scan.indent, pos, end)?;
                run_start = pos;
            }
        }
        self.push_verbatim(run_start, pos)
    }

    /// Remove as much of `indent` as starts this line.
    fn strip_indent(
        &mut self,
        indent: &str,
        line_start: usize,
        end: usize,
    ) -> Result<usize, CodecRefusal> {
        let removed = common_char_prefix(indent, &self.source[line_start..end]).len();
        if removed > 0 {
            let input = authored_range(line_start, line_start + removed);
            self.push_elision(input, ElisionKind::Indentation)?;
        }
        Ok(line_start + removed)
    }

    /// Decode the escape at `pos`, returning the offset after it.
    fn decode_escape(
        &mut self,
        escapes: &EscapePolicy,
        introducer: char,
        pos: usize,
        end: usize,
    ) -> Result<usize, CodecRefusal> {
        let source = self.source;
        let after = pos + introducer.len_utf8();
        match scan_escape(escapes, &source[after..end]) {
            EscapeScan::Invalid { len } => Err(missing_cooked(pos, after + len)),
            EscapeScan::Continuation { len } => {
                let input = authored_range(pos, after + len);
                self.push_elision(input, ElisionKind::LineContinuation)?;
                Ok(after + len)
            }
            EscapeScan::Scalar { cooked, len } => {
                self.push_whole(cooked, pos, after + len, WholeKind::Escape)?;
                Ok(after + len)
            }
            EscapeScan::Numeric { value, len } => {
                let escape_end = after + len;
                let (scalar, whole_end) = match value {
                    0xD800..=0xDBFF => {
                        let (low, low_end) =
                            scan_low_surrogate(escapes, introducer, source, escape_end, end)
                                .ok_or_else(|| {
                                    lone_surrogate(SurrogateKind::High, pos, escape_end)
                                })?;
                        let scalar = 0x1_0000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                        (scalar, low_end)
                    }
                    0xDC00..=0xDFFF => {
                        return Err(lone_surrogate(SurrogateKind::Low, pos, escape_end));
                    }
                    _ => (value, escape_end),
                };
                let cooked =
                    char::from_u32(scalar).ok_or_else(|| missing_cooked(pos, whole_end))?;
                self.push_whole(cooked, pos, whole_end, WholeKind::Escape)?;
                Ok(whole_end)
            }
        }
    }
}

/// Refuse an unescaped closer or hole opener at `pos`.
fn check_syntax_at(
    source: &str,
    scan: &ScanSettings<'_>,
    pos: usize,
    end: usize,
) -> Result<(), CodecRefusal> {
    let rest = &source[pos..end];
    if !scan.close.is_empty() && rest.starts_with(scan.close) {
        return Err(CodecRefusal::UnescapedDelimiter {
            authored: authored_range(pos, pos + scan.close.len()),
        });
    }
    if !scan.opener.is_empty() && rest.starts_with(scan.opener) {
        return Err(CodecRefusal::UndeclaredHole {
            authored: authored_range(pos, pos + scan.opener.len()),
        });
    }
    Ok(())
}

enum EscapeScan {
    Invalid { len: usize },
    Continuation { len: usize },
    Scalar { cooked: char, len: usize },
    Numeric { value: u32, len: usize },
}

/// Classify the text after an introducer. `len` counts bytes after it.
fn scan_escape(escapes: &EscapePolicy, rest: &str) -> EscapeScan {
    let Some(marker) = rest.chars().next() else {
        return EscapeScan::Invalid { len: 0 };
    };
    let continuation = escapes
        .continuations
        .iter()
        .filter(|spelling| !spelling.is_empty() && rest.starts_with(&***spelling))
        .map(|spelling| spelling.len())
        .max();
    if let Some(len) = continuation {
        return EscapeScan::Continuation { len };
    }
    let len = marker.len_utf8();
    if escapes.invalid_markers.contains(&marker) {
        return EscapeScan::Invalid { len };
    }
    if let Some(simple) = escapes.simple.iter().find(|simple| simple.marker == marker) {
        let next = rest[len..].chars().next();
        if next.is_some_and(|next| simple.invalid_before.contains(&next)) {
            return EscapeScan::Invalid { len };
        }
        return EscapeScan::Scalar {
            cooked: simple.cooked,
            len,
        };
    }
    let mut numeric_marker = false;
    for numeric in escapes.numeric.iter().filter(|n| n.marker() == marker) {
        numeric_marker = true;
        if let Some((value, consumed)) = scan_numeric(numeric, &rest[len..]) {
            return EscapeScan::Numeric {
                value,
                len: len + consumed,
            };
        }
    }
    if numeric_marker || escapes.identity == IdentityEscapes::Invalid {
        return EscapeScan::Invalid { len };
    }
    EscapeScan::Scalar {
        cooked: marker,
        len,
    }
}

/// The code point a numeric escape spells and the bytes its digits take.
fn scan_numeric(numeric: &NumericEscape, rest: &str) -> Option<(u32, usize)> {
    match *numeric {
        NumericEscape::Fixed { radix, digits, .. } => {
            let mut value = 0u32;
            let mut chars = rest.chars();
            for _ in 0..digits {
                let digit = chars.next()?.to_digit(radix)?;
                value = value.checked_mul(radix)?.checked_add(digit)?;
            }
            // Every digit of a radix `to_digit` accepts is one ASCII byte.
            (value <= 0x10_FFFF).then_some((value, usize::from(digits)))
        }
        NumericEscape::Braced {
            open, close, radix, ..
        } => {
            let body = rest.strip_prefix(open)?;
            let digits_len = body.find(close)?;
            let digits = &body[..digits_len];
            if digits.is_empty() {
                return None;
            }
            let mut value = 0u32;
            for digit in digits.chars() {
                let digit = digit.to_digit(radix)?;
                value = value.checked_mul(radix)?.checked_add(digit)?;
                if value > 0x10_FFFF {
                    return None;
                }
            }
            Some((value, open.len_utf8() + digits_len + close.len_utf8()))
        }
    }
}

/// A low-surrogate numeric escape immediately at `pos`, with the offset after.
fn scan_low_surrogate(
    escapes: &EscapePolicy,
    introducer: char,
    source: &str,
    pos: usize,
    end: usize,
) -> Option<(u32, usize)> {
    let after = source[pos..end].strip_prefix(introducer)?;
    match scan_escape(escapes, after) {
        EscapeScan::Numeric { value, len } if (0xDC00..=0xDFFF).contains(&value) => {
            Some((value, pos + introducer.len_utf8() + len))
        }
        _ => None,
    }
}

fn missing_cooked(start: usize, end: usize) -> CodecRefusal {
    CodecRefusal::MissingCookedValue(MissingCookedValue {
        authored: Some(authored_range(start, end)),
    })
}

fn lone_surrogate(surrogate: SurrogateKind, start: usize, end: usize) -> CodecRefusal {
    CodecRefusal::NonUnicodeCookedLiteral(NonUnicodeCookedLiteral {
        surrogate,
        authored: Some(authored_range(start, end)),
    })
}

fn terminator_len(rest: &str) -> usize {
    if rest.starts_with("\r\n") {
        2
    } else {
        1
    }
}

/// The longest declared-whitespace prefix common to every non-blank line. A
/// line ending in a hole has content, so it is not blank.
fn common_indent<'s>(
    source: &'s str,
    geometry: &LiteralGeometry,
    whitespace: &[char],
    include_first_line: bool,
    introducer: Option<char>,
) -> &'s str {
    let mut common: Option<&'s str> = None;
    let last_chunk = geometry.chunk_count() - 1;
    for index in 0..=last_chunk {
        let chunk = geometry.chunk(index);
        let (start, end) = (chunk.start().get() as usize, chunk.end().get() as usize);
        let mut consider = |line_start: usize| {
            let line = &source[line_start..end];
            let leading = line
                .find(|ch: char| !whitespace.contains(&ch))
                .unwrap_or(line.len());
            let after = &line[leading..];
            let blank = if after.is_empty() {
                index == last_chunk
            } else {
                after.starts_with(['\n', '\r'])
            };
            if !blank {
                let prefix = &line[..leading];
                common = Some(common.map_or(prefix, |c| common_char_prefix(c, prefix)));
            }
        };
        if index == 0 && include_first_line {
            consider(start);
        }
        let mut pos = start;
        while let Some(ch) = source[pos..end].chars().next() {
            if introducer == Some(ch) {
                pos += ch.len_utf8();
                let rest = &source[pos..end];
                pos += match rest.chars().next() {
                    Some('\r') => terminator_len(rest),
                    Some(escaped) => escaped.len_utf8(),
                    None => 0,
                };
            } else if ch == '\n' || ch == '\r' {
                pos += terminator_len(&source[pos..end]);
                consider(pos);
            } else {
                pos += ch.len_utf8();
            }
        }
    }
    common.unwrap_or("")
}

/// The longest whole-character prefix of `a` that also starts `b`.
fn common_char_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let len = a
        .chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .map(|(left, _)| left.len_utf8())
        .sum();
    &a[..len]
}

fn delimiter_ranges(geometry: &LiteralGeometry) -> (SourceByteRange, SourceByteRange) {
    (
        SourceByteRange::new(geometry.outer().start(), geometry.body().start()),
        SourceByteRange::new(geometry.body().end(), geometry.outer().end()),
    )
}

fn slice(source: &str, range: SourceByteRange) -> &str {
    &source[range.start().get() as usize..range.end().get() as usize]
}

fn offset_of(len: usize) -> Result<EmbeddedByteOffset, CodecRefusal> {
    u32::try_from(len)
        .map(EmbeddedByteOffset::new)
        .map_err(|_| CodecRefusal::InvalidGeometry(CoordError::Overflow))
}

/// `check_request` proved the source fits in `u32`, so every offset into it does.
fn byte_offset(offset: usize) -> SourceByteOffset {
    SourceByteOffset::new(offset as u32)
}

fn authored_range(start: usize, end: usize) -> SourceByteRange {
    SourceByteRange::new(byte_offset(start), byte_offset(end))
}
