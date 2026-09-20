//! Emission correspondence and role-qualified source mappings.
//!
//! Checking text and correspondence come from one [`CodeTransform`] operation
//! record. Binding/use identity and observation roles are attached beside
//! mapping geometry, never recovered from generated text. Generated-text
//! revision is distinct from source-correspondence revision: reused checking
//! text still receives the current transform's source positions.
//!
//! Mapper kinds are TCM1 [`ProjectedClass`] values. [`EmitOp`] lowers to those
//! kinds only where the emit variant's byte geometry already justifies the
//! class. Preprocessor/external chains go through
//! [`CodeTransform::chain_source_map`], not a second map generator.

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ParseKey, SourceId, SourceUnitId, SyntaxProfileId};

use crate::assembly::source_unit::{carrier_revision, carrier_source_id};
pub use crate::code_transform::MappingProduct;
use crate::code_transform::{
    CodeTransform, MappingSpan, ProjectedClass, ProjectedRegion, SourceMapChainError,
};
use crate::ide::template::emit::EmitOp;

use super::{
    mint_snapshot, AdmittedExpressionId, BindingOriginId, ComponentUseId, PlanSnapshotId,
    ProjectionPlan,
};

const TEXT_REV_DOMAIN: &str = "verter.compiler.projection_plan.checking_text_revision.v1";
const CORR_REV_DOMAIN: &str = "verter.compiler.projection_plan.correspondence_revision.v1";

/// Observation channels. Hover/definition/references/edits are the STP9 use
/// participation set. Diagnostic and Feature are intentional anchors that are
/// not edit-safety claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationRole {
    Hover,
    Definition,
    References,
    Edits,
    Diagnostic,
    Feature,
}

impl ObservationRole {
    fn tag(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Definition => "definition",
            Self::References => "references",
            Self::Edits => "edits",
            Self::Diagnostic => "diagnostic",
            Self::Feature => "feature",
        }
    }
}

/// Source binding/use identity independent of mapping geometry.
/// Authored IDs are bound to the [`PlanSnapshotId`] / [`InputBasisId`] they
/// were admitted against.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectionOrigin {
    binding: Option<BindingOriginId>,
    use_id: Option<ComponentUseId>,
    expression: Option<AdmittedExpressionId>,
    snapshot: Option<PlanSnapshotId>,
    input_basis: Option<InputBasisId>,
}

impl ProjectionOrigin {
    #[must_use]
    pub fn new(
        binding: Option<BindingOriginId>,
        use_id: Option<ComponentUseId>,
        expression: Option<AdmittedExpressionId>,
    ) -> Self {
        Self {
            binding,
            use_id,
            expression,
            snapshot: None,
            input_basis: None,
        }
    }

    /// Stamp durable IDs with the plan snapshot they belong to.
    pub fn bind(
        plan: &ProjectionPlan,
        binding: Option<BindingOriginId>,
        use_id: Option<ComponentUseId>,
        expression: Option<AdmittedExpressionId>,
    ) -> Result<Self, EmissionRefusal> {
        let origin = Self {
            binding,
            use_id,
            expression,
            snapshot: Some(plan.snapshot.clone()),
            input_basis: Some(plan.input_basis.clone()),
        };
        if origin.is_authored() && !ids_in_plan(plan, &origin) {
            return Err(EmissionRefusal::UnboundOrigin);
        }
        Ok(origin)
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(None, None, None)
    }

    #[must_use]
    pub fn binding(&self) -> Option<&BindingOriginId> {
        self.binding.as_ref()
    }

    #[must_use]
    pub fn use_id(&self) -> Option<&ComponentUseId> {
        self.use_id.as_ref()
    }

    #[must_use]
    pub fn expression(&self) -> Option<&AdmittedExpressionId> {
        self.expression.as_ref()
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<&PlanSnapshotId> {
        self.snapshot.as_ref()
    }

    #[must_use]
    pub fn input_basis(&self) -> Option<&InputBasisId> {
        self.input_basis.as_ref()
    }

    /// True when this origin names an authored binding, use, or expression.
    #[must_use]
    pub fn is_authored(&self) -> bool {
        self.binding.is_some() || self.use_id.is_some() || self.expression.is_some()
    }
}

/// Edit participation for a carrier preimage. Feature observations are not
/// edit-safety claims; only [`ObservationRole::Edits`] on verbatim geometry
/// produces an edit origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditOrigin {
    origin: ProjectionOrigin,
    carrier: MappingSpan,
}

impl EditOrigin {
    #[must_use]
    pub fn origin(&self) -> &ProjectionOrigin {
        &self.origin
    }

    #[must_use]
    pub fn carrier(&self) -> MappingSpan {
        self.carrier
    }
}

/// One generated observation with a role and a semantic origin. Multiple
/// observations may share an authored origin; returned virtual spans must not
/// overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleQualifiedObservation {
    generated: MappingSpan,
    role: ObservationRole,
    origin: ProjectionOrigin,
}

impl RoleQualifiedObservation {
    #[must_use]
    pub fn new(generated: MappingSpan, role: ObservationRole, origin: ProjectionOrigin) -> Self {
        Self {
            generated,
            role,
            origin,
        }
    }

    #[must_use]
    pub fn generated(&self) -> MappingSpan {
        self.generated
    }

    #[must_use]
    pub fn role(&self) -> ObservationRole {
        self.role
    }

    #[must_use]
    pub fn origin(&self) -> &ProjectionOrigin {
        &self.origin
    }
}

/// Hash of checking text alone. Unchanged text keeps this revision even when
/// source correspondence moves.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CheckingTextRevision(Canonical);

/// Hash of mapping geometry plus observation origins. Independent of checking
/// text: reused generated bytes still get current source positions.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CorrespondenceRevision(Canonical);

macro_rules! rev_debug {
    ($t:ident) => {
        impl core::fmt::Debug for $t {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($t), "({})"), self.0.digest().to_hex())
            }
        }
        impl $t {
            #[must_use]
            pub fn digest_hex(&self) -> String {
                self.0.digest().to_hex()
            }
            #[must_use]
            pub fn digest(&self) -> CanonicalDigest {
                self.0.digest()
            }
        }
    };
}

rev_debug!(CheckingTextRevision);
rev_debug!(CorrespondenceRevision);

/// Why an emission cannot be published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmissionRefusal {
    /// Two returned virtual mapping spans overlap.
    OverlappingVirtualSpans {
        first: MappingSpan,
        second: MappingSpan,
    },
    /// Scaffolding with no authored preimage claimed an authored location.
    SyntheticAuthoredLocation { generated: MappingSpan },
    /// Unchanged checking text reused pre-edit source positions.
    StaleMap,
    /// An observation names generated bytes the mapping does not account for.
    ObservationOutsideMapping { generated: MappingSpan },
    /// Verbatim Identity/Relocated bytes do not equal the carrier slice.
    RoundtripMismatch {
        generated: MappingSpan,
        carrier: MappingSpan,
    },
    /// Edit origin requested for a role or class that is not edit-safe.
    EditNotVerbatim {
        generated: MappingSpan,
        role: ObservationRole,
    },
    /// Authored origin is not a member of the emission's input snapshot.
    UnboundOrigin,
    /// Observation span is not on UTF-8 character boundaries.
    ObservationNotOnCharBoundary { generated: MappingSpan },
}

/// Checking text plus correspondence from one transform operation record,
/// with role-qualified origins attached independently of geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionEmission {
    checking_text: String,
    text_revision: CheckingTextRevision,
    correspondence_revision: CorrespondenceRevision,
    mapping: MappingProduct,
    observations: Vec<RoleQualifiedObservation>,
    snapshot: Option<PlanSnapshotId>,
    input_basis: Option<InputBasisId>,
    mint: Option<SnapshotMint>,
}

impl ProjectionEmission {
    /// Derive checking text and correspondence from one transform. Mapping is
    /// always [`MappingProduct::of`] the current record. Authored origins
    /// require [`Self::from_plan`].
    pub fn from_operation(
        transform: &CodeTransform<'_>,
        observations: Vec<RoleQualifiedObservation>,
    ) -> Result<Self, EmissionRefusal> {
        let checking_text = transform.build_string();
        let mapping = MappingProduct::of(transform);
        assemble(checking_text, mapping, observations, None)
    }

    /// Derive an emission bound to `plan`'s snapshot. `canonical_id` must be
    /// the lineage the plan was observed against; transform original bytes
    /// must mint that same snapshot.
    pub fn from_plan(
        transform: &CodeTransform<'_>,
        plan: &ProjectionPlan,
        canonical_id: &str,
        observations: Vec<RoleQualifiedObservation>,
    ) -> Result<Self, EmissionRefusal> {
        if !plan_matches_transform(plan, canonical_id, transform.original()) {
            return Err(EmissionRefusal::UnboundOrigin);
        }
        let checking_text = transform.build_string();
        let mapping = MappingProduct::of(transform);
        assemble(
            checking_text,
            mapping,
            observations,
            Some(binding_from_plan(plan, canonical_id)),
        )
    }

    /// Attempt to keep a previous mapping when checking text is unchanged.
    /// Unchanged text with moved source positions is [`EmissionRefusal::StaleMap`].
    ///
    /// A previous plan binding is retained only when `current` is the same
    /// snapshot and input basis, and the current original bytes still mint
    /// that snapshot. Authored observations still have to be members of
    /// `current`. Reminting the previous identity against current bytes
    /// cannot witness a different canonical or parse identity. Equal checking
    /// text and geometry cannot keep a stale source identity.
    pub fn reuse_mapping(
        previous: &Self,
        transform: &CodeTransform<'_>,
        current: Option<(&ProjectionPlan, &str)>,
        observations: Vec<RoleQualifiedObservation>,
    ) -> Result<Self, EmissionRefusal> {
        if let Some((plan, canonical_id)) = current {
            if !plan_matches_transform(plan, canonical_id, transform.original()) {
                return Err(EmissionRefusal::StaleMap);
            }
        }
        if previous.snapshot.is_some() || previous.mint.is_some() {
            let Some((plan, _)) = current else {
                return Err(EmissionRefusal::StaleMap);
            };
            if previous.snapshot.as_ref() != Some(&plan.snapshot)
                || previous.input_basis.as_ref() != Some(&plan.input_basis)
            {
                return Err(EmissionRefusal::StaleMap);
            }
        }
        if let Some(mint) = previous.mint.as_ref() {
            if previous.snapshot.as_ref() != Some(&mint.remint(transform.original())) {
                return Err(EmissionRefusal::StaleMap);
            }
        }
        let checking_text = transform.build_string();
        let mapping = MappingProduct::of(transform);
        if checking_text == previous.checking_text && mapping != previous.mapping {
            return Err(EmissionRefusal::StaleMap);
        }
        // Current plan membership is the reuse origin check. Previous snapshot
        // equality cannot admit an authored ID absent from this plan.
        // A None `current` is reachable only for an unbound `previous`; an unbound
        // emission carries no snapshot, input basis, or mint to preserve.
        assemble(
            checking_text,
            mapping,
            observations,
            current.map(|(plan, canonical_id)| binding_from_plan(plan, canonical_id)),
        )
    }

    #[must_use]
    pub fn checking_text(&self) -> &str {
        &self.checking_text
    }

    #[must_use]
    pub fn text_revision(&self) -> &CheckingTextRevision {
        &self.text_revision
    }

    #[must_use]
    pub fn correspondence_revision(&self) -> &CorrespondenceRevision {
        &self.correspondence_revision
    }

    #[must_use]
    pub fn mapping(&self) -> &MappingProduct {
        &self.mapping
    }

    #[must_use]
    pub fn observations(&self) -> &[RoleQualifiedObservation] {
        &self.observations
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<&PlanSnapshotId> {
        self.snapshot.as_ref()
    }

    #[must_use]
    pub fn input_basis(&self) -> Option<&InputBasisId> {
        self.input_basis.as_ref()
    }

    /// Identity and Relocated regions must equal the carrier slice, including
    /// Unicode scalar values and CRLF byte pairs.
    pub fn roundtrip_verbatim(&self, source: &str) -> Result<(), EmissionRefusal> {
        for region in self.mapping.projected() {
            match region.class {
                ProjectedClass::Identity | ProjectedClass::Relocated => {
                    let Some(carrier) = region.carrier else {
                        return Err(EmissionRefusal::RoundtripMismatch {
                            generated: region.generated,
                            carrier: MappingSpan { start: 0, end: 0 },
                        });
                    };
                    let generated = try_slice_bytes(&self.checking_text, region.generated);
                    let authored = try_slice_bytes(source, carrier);
                    if generated.is_none() || authored.is_none() || generated != authored {
                        return Err(EmissionRefusal::RoundtripMismatch {
                            generated: region.generated,
                            carrier,
                        });
                    }
                }
                ProjectedClass::Rewritten | ProjectedClass::Synthesized => {}
            }
        }
        Ok(())
    }

    /// Edit origin for an observation. Only an [`ObservationRole::Edits`]
    /// observation admitted into this emission, on Identity/Relocated
    /// geometry with a carrier preimage.
    pub fn edit_origin(
        &self,
        observation: &RoleQualifiedObservation,
    ) -> Result<EditOrigin, EmissionRefusal> {
        if !self
            .observations
            .iter()
            .any(|admitted| admitted == observation)
        {
            return Err(EmissionRefusal::EditNotVerbatim {
                generated: observation.generated,
                role: observation.role,
            });
        }
        if observation.role != ObservationRole::Edits {
            return Err(EmissionRefusal::EditNotVerbatim {
                generated: observation.generated,
                role: observation.role,
            });
        }
        if !span_on_char_boundaries(&self.checking_text, observation.generated)
            || span_is_wholly_synthesized(&self.mapping, observation.generated)
            || !span_is_verbatim(&self.mapping, observation.generated)
        {
            return Err(EmissionRefusal::EditNotVerbatim {
                generated: observation.generated,
                role: observation.role,
            });
        }
        let Some(carrier) = carrier_for_generated(&self.mapping, observation.generated) else {
            return Err(EmissionRefusal::EditNotVerbatim {
                generated: observation.generated,
                role: observation.role,
            });
        };
        Ok(EditOrigin {
            origin: observation.origin.clone(),
            carrier,
        })
    }
}

/// Lower an [`EmitOp`] to mapper classes only where the variant's byte
/// geometry already is those classes. Never invents a correspondence.
///
/// [`EmitOp::PreserveOriginal`] is a no-op: leftover Original chunks are
/// classified by the completed [`MappingProduct`] walk (Identity vs
/// Relocated depends on whether an earlier move advanced the authored
/// high-water mark). Other variants lower from local geometry alone.
///
/// [`EmitOp::InsertMapped`] follows the TCM1 partition: bytes before
/// `content_offset` are [`ProjectedClass::Synthesized`]; the suffix is
/// [`ProjectedClass::Rewritten`]. An offset equal to the text length
/// produces only Synthesized regions.
///
/// Offset basis: [`EmitOp::PreserveOriginal`] returns absolute generated
/// offsets from the completed [`MappingProduct`]. Every other variant
/// returns spans local to the emitted text (or the moved slice), starting
/// at `0`. Callers must not mix the two spaces.
#[must_use]
pub fn projected_class_for_emit_op(
    op: &EmitOp<'_>,
    transform: &CodeTransform<'_>,
) -> Vec<(MappingSpan, ProjectedClass)> {
    projected_class_for_emit_op_with(op, &MappingProduct::of(transform))
}

/// Same lowering against a mapping the caller already built once per transform.
#[must_use]
pub fn projected_class_for_emit_op_with(
    op: &EmitOp<'_>,
    mapping: &MappingProduct,
) -> Vec<(MappingSpan, ProjectedClass)> {
    match op {
        EmitOp::PreserveOriginal { source } => preserved_classes_from_mapping(
            mapping,
            MappingSpan {
                start: source.start.0,
                end: source.end.0,
            },
        ),
        EmitOp::MoveOriginal { source, .. } => moved_classes_from_mapping(
            mapping,
            MappingSpan {
                start: source.start.0,
                end: source.end.0,
            },
        ),
        EmitOp::InsertMapped {
            text,
            content_offset,
            ..
        } => mapped_insertion_classes(text.as_str().len() as u32, content_offset.0),
        EmitOp::InsertUnmapped { text, .. } | EmitOp::OverwriteSyntheticBoundary { text, .. } => {
            vec![(
                MappingSpan {
                    start: 0,
                    end: text.as_str().len() as u32,
                },
                ProjectedClass::Synthesized,
            )]
        }
    }
}

fn mapped_insertion_classes(len: u32, content_offset: u32) -> Vec<(MappingSpan, ProjectedClass)> {
    let offset = content_offset.min(len);
    let mut regions = Vec::new();
    if offset > 0 {
        regions.push((
            MappingSpan {
                start: 0,
                end: offset,
            },
            ProjectedClass::Synthesized,
        ));
    }
    if offset < len {
        regions.push((
            MappingSpan {
                start: offset,
                end: len,
            },
            ProjectedClass::Rewritten,
        ));
    }
    if regions.is_empty() {
        regions.push((
            MappingSpan { start: 0, end: 0 },
            ProjectedClass::Synthesized,
        ));
    }
    regions
}

/// Compose an upstream preprocessor/external map through the transform that
/// owns the current bytes. No independent map generator.
pub fn compose_source_chain(
    transform: &CodeTransform<'_>,
    upstream: &oxc_sourcemap::SourceMap<'_>,
) -> Result<oxc_sourcemap::SourceMap<'static>, SourceMapChainError> {
    transform.chain_source_map(upstream)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotMint {
    source_id: SourceId,
    template_unit: SourceUnitId,
    parse_key: Option<ParseKey>,
    syntax_profile: Option<SyntaxProfileId>,
}

impl SnapshotMint {
    fn from_plan(plan: &ProjectionPlan, canonical_id: &str) -> Self {
        Self {
            source_id: carrier_source_id(canonical_id),
            template_unit: plan.template_unit.clone(),
            parse_key: plan.parse_key.clone(),
            syntax_profile: plan.syntax_profile.clone(),
        }
    }

    fn remint(&self, source: &str) -> PlanSnapshotId {
        mint_snapshot(
            &self.source_id,
            &carrier_revision(source),
            &self.template_unit,
            self.parse_key.as_ref(),
            self.syntax_profile.as_ref(),
        )
    }
}

struct EmissionBinding<'a> {
    snapshot: &'a PlanSnapshotId,
    input_basis: &'a InputBasisId,
    plan: &'a ProjectionPlan,
    mint: SnapshotMint,
}

fn binding_from_plan<'a>(plan: &'a ProjectionPlan, canonical_id: &str) -> EmissionBinding<'a> {
    EmissionBinding {
        snapshot: &plan.snapshot,
        input_basis: &plan.input_basis,
        plan,
        mint: SnapshotMint::from_plan(plan, canonical_id),
    }
}

fn assemble(
    checking_text: String,
    mapping: MappingProduct,
    mut observations: Vec<RoleQualifiedObservation>,
    binding: Option<EmissionBinding<'_>>,
) -> Result<ProjectionEmission, EmissionRefusal> {
    for observation in &observations {
        if observation.generated.start > observation.generated.end
            || observation.generated.end > mapping.projected_len()
            || observation.generated.start > mapping.projected_len()
        {
            return Err(EmissionRefusal::ObservationOutsideMapping {
                generated: observation.generated,
            });
        }
        if !span_on_char_boundaries(&checking_text, observation.generated) {
            return Err(EmissionRefusal::ObservationNotOnCharBoundary {
                generated: observation.generated,
            });
        }
    }
    observations.sort_by_key(|obs| (obs.generated.start, obs.generated.end, obs.role.tag()));
    if let Some((first, second)) = overlapping_virtual_spans(&observations) {
        return Err(EmissionRefusal::OverlappingVirtualSpans { first, second });
    }
    for observation in &observations {
        if observation.origin.is_authored()
            && span_is_wholly_synthesized(&mapping, observation.generated)
        {
            return Err(EmissionRefusal::SyntheticAuthoredLocation {
                generated: observation.generated,
            });
        }
        if observation.origin.is_authored() {
            let Some(binding) = binding.as_ref() else {
                return Err(EmissionRefusal::UnboundOrigin);
            };
            if !origin_matches_binding(binding, &observation.origin) {
                return Err(EmissionRefusal::UnboundOrigin);
            }
        }
    }
    let text_revision = mint_text_revision(&checking_text);
    let correspondence_revision = mint_correspondence_revision(
        &mapping,
        &observations,
        binding
            .as_ref()
            .map(|binding| (binding.snapshot, binding.input_basis)),
    );
    Ok(ProjectionEmission {
        checking_text,
        text_revision,
        correspondence_revision,
        mapping,
        observations,
        snapshot: binding.as_ref().map(|binding| binding.snapshot.clone()),
        input_basis: binding.as_ref().map(|binding| binding.input_basis.clone()),
        mint: binding.as_ref().map(|binding| binding.mint.clone()),
    })
}

fn overlapping_virtual_spans(
    observations: &[RoleQualifiedObservation],
) -> Option<(MappingSpan, MappingSpan)> {
    overlapping_virtual_spans_with_work(observations).0
}

/// Sweep already-sorted observations. Overlap is a generated-span property;
/// role does not create a second population.
fn overlapping_virtual_spans_with_work(
    observations: &[RoleQualifiedObservation],
) -> (Option<(MappingSpan, MappingSpan)>, usize) {
    let mut last: Option<&RoleQualifiedObservation> = None;
    let mut work = 0usize;
    for observation in observations {
        if let Some(previous) = last {
            work += 1;
            if spans_overlap(previous.generated, observation.generated) {
                return (Some((previous.generated, observation.generated)), work);
            }
            if observation.generated.end >= previous.generated.end {
                last = Some(observation);
            }
        } else {
            last = Some(observation);
        }
    }
    (None, work)
}

fn ids_in_plan(plan: &ProjectionPlan, origin: &ProjectionOrigin) -> bool {
    if let Some(binding) = origin.binding.as_ref() {
        if !plan.origins.iter().any(|row| row.id == *binding) {
            return false;
        }
    }
    if let Some(use_id) = origin.use_id.as_ref() {
        if !plan.uses.iter().any(|row| row.id == *use_id) {
            return false;
        }
    }
    if let Some(expression) = origin.expression.as_ref() {
        if plan.expression(expression).is_none() {
            return false;
        }
    }
    true
}

fn origin_matches_plan(plan: &ProjectionPlan, origin: &ProjectionOrigin) -> bool {
    origin.snapshot.as_ref() == Some(&plan.snapshot)
        && origin.input_basis.as_ref() == Some(&plan.input_basis)
        && ids_in_plan(plan, origin)
}

fn origin_matches_binding(binding: &EmissionBinding<'_>, origin: &ProjectionOrigin) -> bool {
    origin_matches_plan(binding.plan, origin)
}

fn plan_matches_transform(plan: &ProjectionPlan, canonical_id: &str, source: &str) -> bool {
    let source_id = carrier_source_id(canonical_id);
    let revision = carrier_revision(source);
    let snapshot = mint_snapshot(
        &source_id,
        &revision,
        &plan.template_unit,
        plan.parse_key.as_ref(),
        plan.syntax_profile.as_ref(),
    );
    snapshot == plan.snapshot
}

fn preserved_classes_from_mapping(
    mapping: &MappingProduct,
    carrier: MappingSpan,
) -> Vec<(MappingSpan, ProjectedClass)> {
    if carrier.start >= carrier.end {
        return Vec::new();
    }
    let mut out = Vec::new();
    for region in mapping.projected() {
        let Some(region_carrier) = region.carrier else {
            continue;
        };
        let overlap_start = carrier.start.max(region_carrier.start);
        let overlap_end = carrier.end.min(region_carrier.end);
        if overlap_start >= overlap_end {
            continue;
        }
        let generated = if matches!(
            region.class,
            ProjectedClass::Identity | ProjectedClass::Relocated
        ) && region.generated.len() == region_carrier.len()
        {
            let local = overlap_start - region_carrier.start;
            let len = overlap_end - overlap_start;
            MappingSpan {
                start: region.generated.start + local,
                end: region.generated.start + local + len,
            }
        } else if overlap_start == region_carrier.start && overlap_end == region_carrier.end {
            // Rewritten is region-to-region, including equal generated and
            // carrier lengths. A partial carrier overlap has no exact
            // generated image; offset arithmetic can split a UTF-8 scalar.
            region.generated
        } else {
            continue;
        };
        out.push((generated, region.class));
    }
    out
}

fn moved_classes_from_mapping(
    mapping: &MappingProduct,
    carrier: MappingSpan,
) -> Vec<(MappingSpan, ProjectedClass)> {
    if carrier.start >= carrier.end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut local_offset = 0u32;
    for region in mapping.projected() {
        let Some(region_carrier) = region.carrier else {
            continue;
        };
        let overlap_start = carrier.start.max(region_carrier.start);
        let overlap_end = carrier.end.min(region_carrier.end);
        if overlap_start >= overlap_end {
            continue;
        }
        let (len, class) = if matches!(
            region.class,
            ProjectedClass::Identity | ProjectedClass::Relocated
        ) && region.generated.len() == region_carrier.len()
        {
            let len = overlap_end - overlap_start;
            (len, ProjectedClass::Relocated)
        } else if overlap_start == region_carrier.start && overlap_end == region_carrier.end {
            (region.generated.len(), region.class)
        } else {
            continue;
        };
        if len > 0 {
            out.push((
                MappingSpan {
                    start: local_offset,
                    end: local_offset + len,
                },
                class,
            ));
            local_offset += len;
        }
    }
    out
}

fn spans_overlap(a: MappingSpan, b: MappingSpan) -> bool {
    if a.is_empty() && b.is_empty() {
        a.start == b.start
    } else if a.is_empty() {
        a.start >= b.start && a.start < b.end
    } else if b.is_empty() {
        b.start >= a.start && b.start < a.end
    } else {
        a.start < b.end && b.start < a.end
    }
}

#[cfg(test)]
fn slice_bytes(text: &str, span: MappingSpan) -> &str {
    let start = (span.start as usize).min(text.len());
    let end = (span.end as usize).min(text.len());
    if start <= end && text.is_char_boundary(start) && text.is_char_boundary(end) {
        &text[start..end]
    } else {
        ""
    }
}

fn try_slice_bytes(text: &str, span: MappingSpan) -> Option<&str> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start > end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    Some(&text[start..end])
}

fn span_on_char_boundaries(text: &str, span: MappingSpan) -> bool {
    let start = span.start as usize;
    let end = span.end as usize;
    start <= end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end)
}

fn span_is_exact_insertion_anchor(mapping: &MappingProduct, span: MappingSpan) -> bool {
    span.start == span.end
        && mapping
            .insertion_anchors()
            .iter()
            .any(|anchor| anchor.projected == span.start)
}

fn span_is_wholly_synthesized(mapping: &MappingProduct, span: MappingSpan) -> bool {
    if span.start > span.end || span.end > mapping.projected_len() {
        return true;
    }
    if span.start == span.end {
        if span_is_exact_insertion_anchor(mapping, span) {
            return false;
        }
        return mapping
            .projected_at(span.start)
            .is_none_or(|region| region.class == ProjectedClass::Synthesized);
    }
    let mut offset = span.start;
    while offset < span.end {
        let Some(region) = mapping.projected_at(offset) else {
            return true;
        };
        if region.class != ProjectedClass::Synthesized {
            return false;
        }
        offset = region.generated.end.max(offset + 1);
    }
    true
}

fn span_is_verbatim(mapping: &MappingProduct, span: MappingSpan) -> bool {
    if span.end > mapping.projected_len() {
        return false;
    }
    let mut offset = span.start;
    if span.is_empty() {
        return mapping.projected_at(span.start).is_some_and(|region| {
            matches!(
                region.class,
                ProjectedClass::Identity | ProjectedClass::Relocated
            )
        });
    }
    while offset < span.end {
        let Some(region) = mapping.projected_at(offset) else {
            return false;
        };
        if !matches!(
            region.class,
            ProjectedClass::Identity | ProjectedClass::Relocated
        ) {
            return false;
        }
        offset = region.generated.end.max(offset + 1);
    }
    true
}

fn carrier_offset(region: &ProjectedRegion, generated: u32) -> Option<u32> {
    let carrier = region.carrier?;
    let local = generated.checked_sub(region.generated.start)?;
    if local > region.generated.len() || local > carrier.len() {
        return None;
    }
    Some(carrier.start + local)
}

fn carrier_for_generated(mapping: &MappingProduct, span: MappingSpan) -> Option<MappingSpan> {
    if span.end > mapping.projected_len() || span.start > span.end {
        return None;
    }
    if span.is_empty() {
        let region = mapping.projected_at(span.start)?;
        if !matches!(
            region.class,
            ProjectedClass::Identity | ProjectedClass::Relocated
        ) {
            return None;
        }
        let point = carrier_offset(region, span.start)?;
        return Some(MappingSpan {
            start: point,
            end: point,
        });
    }

    let mut offset = span.start;
    let mut carrier_start = None;
    let mut expected_next: Option<u32> = None;
    while offset < span.end {
        let region = mapping.projected_at(offset)?;
        if !matches!(
            region.class,
            ProjectedClass::Identity | ProjectedClass::Relocated
        ) {
            return None;
        }
        let overlap_end = span.end.min(region.generated.end);
        if overlap_end <= offset {
            return None;
        }
        let piece_start = carrier_offset(region, offset)?;
        let piece_end = carrier_offset(region, overlap_end)?;
        if piece_end < piece_start {
            return None;
        }
        if let Some(expected) = expected_next {
            if piece_start != expected {
                return None;
            }
        } else {
            carrier_start = Some(piece_start);
        }
        expected_next = Some(piece_end);
        offset = overlap_end;
    }
    Some(MappingSpan {
        start: carrier_start?,
        end: expected_next?,
    })
}

fn mint_text_revision(checking_text: &str) -> CheckingTextRevision {
    let mut encoder = CanonicalEncoder::new(TEXT_REV_DOMAIN);
    encoder.field_str(1, checking_text);
    CheckingTextRevision(Canonical::from_encoder(&encoder))
}

fn mint_correspondence_revision(
    mapping: &MappingProduct,
    observations: &[RoleQualifiedObservation],
    snapshot: Option<(&PlanSnapshotId, &InputBasisId)>,
) -> CorrespondenceRevision {
    let mut encoder = CanonicalEncoder::new(CORR_REV_DOMAIN);
    encoder.field_u32(1, mapping.projected_len());
    encoder.field_u32(2, mapping.carrier_len());
    for (index, region) in mapping.projected().iter().enumerate() {
        encoder.field_u32(3, index as u32);
        encoder.field_u32(4, region.generated.start);
        encoder.field_u32(5, region.generated.end);
        encoder.field_u32(6, projected_discriminant(region.class));
        match region.carrier {
            Some(span) => {
                encoder.field_u32(7, 1);
                encoder.field_u32(8, span.start);
                encoder.field_u32(9, span.end);
            }
            None => {
                encoder.field_u32(7, 0);
            }
        }
    }
    for region in mapping.carrier() {
        encoder.field_u32(10, region.source.start);
        encoder.field_u32(11, region.source.end);
        encoder.field_u32(12, carrier_discriminant(region.class));
    }
    for observation in observations {
        encoder.field_u32(13, observation.generated.start);
        encoder.field_u32(14, observation.generated.end);
        encoder.field_str(15, observation.role.tag());
        encoder.field_option(
            16,
            observation
                .origin
                .binding
                .as_ref()
                .map(BindingOriginId::canonical_bytes),
        );
        encoder.field_option(
            17,
            observation
                .origin
                .use_id
                .as_ref()
                .map(ComponentUseId::canonical_bytes),
        );
        encoder.field_option(
            18,
            observation
                .origin
                .expression
                .as_ref()
                .map(AdmittedExpressionId::canonical_bytes),
        );
    }
    for (index, anchor) in mapping.insertion_anchors().iter().enumerate() {
        encoder.field_u32(19, index as u32);
        encoder.field_u32(20, anchor.projected);
        encoder.field_u32(21, anchor.carrier);
    }
    match snapshot {
        Some((snapshot, input_basis)) => {
            encoder.field_u32(22, 1);
            encoder.field_bytes(23, snapshot.canonical_bytes());
            encoder.field_bytes(24, input_basis.canonical_bytes());
        }
        None => {
            encoder.field_u32(22, 0);
        }
    }
    CorrespondenceRevision(Canonical::from_encoder(&encoder))
}

fn projected_discriminant(class: ProjectedClass) -> u32 {
    match class {
        ProjectedClass::Identity => 0,
        ProjectedClass::Relocated => 1,
        ProjectedClass::Rewritten => 2,
        ProjectedClass::Synthesized => 3,
    }
}

fn carrier_discriminant(class: crate::code_transform::CarrierClass) -> u32 {
    match class {
        crate::code_transform::CarrierClass::Identity => 0,
        crate::code_transform::CarrierClass::Relocated => 1,
        crate::code_transform::CarrierClass::Rewritten => 2,
        crate::code_transform::CarrierClass::Elided => 3,
    }
}

const _: fn() = || {
    fn assert_plain_owned<T: 'static + Send + Sync + Clone + Eq + std::fmt::Debug>() {}
    assert_plain_owned::<ProjectionEmission>();
    assert_plain_owned::<ProjectionOrigin>();
    assert_plain_owned::<ObservationRole>();
    assert_plain_owned::<EditOrigin>();
    assert_plain_owned::<MappingProduct>();
};

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

    use super::*;
    use crate::code_transform::CarrierClass;
    use crate::framework_common::projection_plan::{
        build_projection_plan, plan_from_source, BindingOriginId, ExpressionKind, PlanInput,
        ProjectionPlan,
    };
    use crate::ide::template::emit::EmitText;
    use verter_identity::encoding::CanonicalEncode;

    const ROLE_SFC: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const items = [1];\n",
        "</script>\n",
        "<template>\n",
        "  <div v-for=\"item in items\">{{ item }}</div>\n",
        "</template>\n",
    );

    const VIEW_PROP_SFC: &str = concat!(
        "<script>\n",
        "export default {\n",
        "  data() { return { count: 1 } }\n",
        "}\n",
        "</script>\n",
        "<template>\n",
        "  <div>{{ count }}</div>\n",
        "</template>\n",
    );

    const VIEW_PROPERTY_PREFIX: &str = "___VERTER___instance.";
    const ROLE_CANONICAL: &str = "file:///stp10-role.vue";

    fn role_plan() -> ProjectionPlan {
        plan_from_source(ROLE_CANONICAL, ROLE_SFC)
    }

    fn binding_item_in(plan: &ProjectionPlan) -> BindingOriginId {
        plan.origins
            .iter()
            .find(|origin| origin.name == "item")
            .expect("v-for alias")
            .id
            .clone()
    }

    fn authored_in(plan: &ProjectionPlan) -> ProjectionOrigin {
        ProjectionOrigin::bind(plan, Some(binding_item_in(plan)), None, None).expect("item in plan")
    }

    fn vfor_alias_span(source: &str) -> MappingSpan {
        let start = source.find("item in items").expect("v-for alias") as u32;
        MappingSpan {
            start,
            end: start + 4,
        }
    }

    #[test]
    fn stp10_roundtrip_verbatim_unicode_and_crlf() {
        let allocator = Allocator::default();
        let source = "café\r\n日本語\r\n";
        let ct = CodeTransform::new(source, &allocator);
        let emission = ProjectionEmission::from_operation(&ct, Vec::new()).expect("identity emit");
        emission.roundtrip_verbatim(source).expect("unicode+CRLF");
        assert_eq!(emission.checking_text(), source);
        assert_eq!(
            emission.mapping().projected()[0].class,
            ProjectedClass::Identity
        );
    }

    #[test]
    fn stp10_role_view_property_and_binding_share_origin_not_symbol() {
        let allocator = Allocator::default();
        let source = VIEW_PROP_SFC;
        let plan = plan_from_source(ROLE_CANONICAL, source);
        let expression = plan
            .expressions()
            .iter()
            .find(|occurrence| occurrence.kind == ExpressionKind::Interpolation)
            .expect("authored count interpolation");
        let origin = ProjectionOrigin::bind(&plan, None, None, Some(expression.id.clone()))
            .expect("interpolation in plan");
        let script_count = source.find("count").expect("original binding") as u32;
        let template_count = source.rfind("count").expect("view property ident") as u32;
        assert_ne!(script_count, template_count);
        let mut ct = CodeTransform::new(source, &allocator);
        ct.prepend_left(template_count, VIEW_PROPERTY_PREFIX);
        let prefix_len = VIEW_PROPERTY_PREFIX.len() as u32;
        let view_span = MappingSpan {
            start: template_count,
            end: template_count + prefix_len + 5,
        };
        let binding_span = MappingSpan {
            start: script_count,
            end: script_count + 5,
        };
        let view =
            RoleQualifiedObservation::new(view_span, ObservationRole::Feature, origin.clone());
        let original = RoleQualifiedObservation::new(
            binding_span,
            ObservationRole::Definition,
            origin.clone(),
        );
        let edits = RoleQualifiedObservation::new(binding_span, ObservationRole::Edits, origin);
        let emission =
            ProjectionEmission::from_plan(&ct, &plan, ROLE_CANONICAL, vec![view.clone(), original])
                .expect("view property and original binding of one authored token");
        assert!(
            emission
                .checking_text()
                .contains("___VERTER___instance.count"),
            "view property must be the instance-prefixed generated symbol: {}",
            emission.checking_text()
        );
        assert_eq!(slice_bytes(emission.checking_text(), binding_span), "count");
        let view_obs = emission
            .observations()
            .iter()
            .find(|obs| obs.role() == ObservationRole::Feature)
            .expect("view property");
        let binding_obs = emission
            .observations()
            .iter()
            .find(|obs| obs.role() == ObservationRole::Definition)
            .expect("original binding");
        assert_eq!(
            view_obs.origin().expression(),
            binding_obs.origin().expression()
        );
        assert_ne!(
            view_obs.generated(),
            binding_obs.generated(),
            "view property and original binding are not one generated TS span"
        );
        assert_eq!(view_obs.generated(), view_span);
        assert_eq!(binding_obs.generated(), binding_span);
        let prefix_region = emission
            .mapping()
            .projected_at(view_span.start)
            .expect("instance prefix");
        let ident_region = emission
            .mapping()
            .projected_at(view_span.start + prefix_len)
            .expect("view property ident");
        let binding_region = emission
            .mapping()
            .projected_at(binding_span.start)
            .expect("original binding");
        assert_eq!(prefix_region.class, ProjectedClass::Synthesized);
        assert_eq!(ident_region.class, ProjectedClass::Identity);
        assert_eq!(binding_region.class, ProjectedClass::Identity);
        assert_eq!(
            ident_region.carrier.map(|span| span.start),
            Some(template_count)
        );
        assert!(
            binding_region
                .carrier
                .is_some_and(|span| { span.start <= script_count && script_count + 5 <= span.end }),
            "script count must sit in the original-binding Identity region: {:?}",
            binding_region.carrier
        );
        assert_ne!(
            ident_region.generated, binding_region.generated,
            "template ident and script binding are distinct generated symbols"
        );
        assert_ne!(
            ident_region.carrier, binding_region.carrier,
            "template ident and script binding are distinct carrier targets of one origin"
        );
        let edit_emission =
            ProjectionEmission::from_plan(&ct, &plan, ROLE_CANONICAL, vec![edits.clone()])
                .expect("edit participation is a separate observation set");
        let edit = edit_emission
            .edit_origin(&edits)
            .expect("original binding edits");
        assert_eq!(edit.carrier(), binding_span);
        assert!(
            matches!(
                emission.edit_origin(&view),
                Err(EmissionRefusal::EditNotVerbatim { .. })
            ),
            "feature participation is not edit safety"
        );
        let view_edits = RoleQualifiedObservation::new(
            view_span,
            ObservationRole::Edits,
            view_obs.origin().clone(),
        );
        assert!(
            matches!(
                emission.edit_origin(&view_edits),
                Err(EmissionRefusal::EditNotVerbatim { .. })
            ),
            "mixed synthesized prefix is not an admitted edit-safe span"
        );
        let forged =
            RoleQualifiedObservation::new(view_span, ObservationRole::Edits, view.origin().clone());
        assert!(
            matches!(
                emission.edit_origin(&forged),
                Err(EmissionRefusal::EditNotVerbatim { .. })
            ),
            "forged Edits role on a Feature observation is not admitted"
        );
    }

    #[test]
    fn stp10_overlap_rejects_overlapping_virtual_spans() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let ct = CodeTransform::new(source, &allocator);
        let origin = ProjectionOrigin::empty();
        let clean = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 3 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 3, end: 6 },
                    ObservationRole::Definition,
                    origin.clone(),
                ),
            ],
        );
        assert!(clean.is_ok(), "{clean:?}");
        let dirty = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 4 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 2, end: 6 },
                    ObservationRole::Definition,
                    origin.clone(),
                ),
            ],
        );
        assert!(
            matches!(dirty, Err(EmissionRefusal::OverlappingVirtualSpans { .. })),
            "{dirty:?}"
        );
        let cross_role = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 1 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 1 },
                    ObservationRole::Edits,
                    origin,
                ),
            ],
        );
        assert!(
            matches!(
                cross_role,
                Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "Hover and Edits on the same generated span overlap: {cross_role:?}"
        );
    }

    #[test]
    fn stp10_stale_map_rejects_reused_pre_edit_positions() {
        let allocator = Allocator::default();
        let before = CodeTransform::new("foo", &allocator);
        let previous = ProjectionEmission::from_operation(&before, Vec::new()).expect("before");
        let mut after = CodeTransform::new("xfoo", &allocator);
        after.overwrite_unmapped(0, 1, "");
        assert_eq!(after.build_string(), "foo");
        assert_eq!(after.build_string(), previous.checking_text());
        let current = MappingProduct::of(&after);
        let previous_identity = previous
            .mapping()
            .projected()
            .iter()
            .find(|region| region.class == ProjectedClass::Identity)
            .and_then(|region| region.carrier);
        let current_identity = current
            .projected()
            .iter()
            .find(|region| region.class == ProjectedClass::Identity)
            .and_then(|region| region.carrier);
        assert_ne!(
            current_identity, previous_identity,
            "eliding the leading byte must move the surviving identity preimage"
        );
        let dirty = ProjectionEmission::reuse_mapping(&previous, &after, None, Vec::new());
        assert!(matches!(dirty, Err(EmissionRefusal::StaleMap)), "{dirty:?}");
        let clean = ProjectionEmission::from_operation(&after, Vec::new()).expect("current map");
        assert_eq!(clean.text_revision(), previous.text_revision());
        assert_ne!(
            clean.correspondence_revision(),
            previous.correspondence_revision()
        );
        assert_eq!(
            clean.mapping().carrier_at(0).map(|region| region.class),
            Some(CarrierClass::Elided)
        );
    }

    #[test]
    fn stp10_synthetic_rejects_authored_location_without_preimage() {
        let allocator = Allocator::default();
        let plan = role_plan();
        let origin = authored_in(&plan);
        let mut ct = CodeTransform::new(ROLE_SFC, &allocator);
        ct.prepend("/* scaffold */");
        let prefix = "/* scaffold */".len() as u32;
        let alias = vfor_alias_span(ROLE_SFC);
        let clean_span = MappingSpan {
            start: alias.start + prefix,
            end: alias.end + prefix,
        };
        let clean = ProjectionEmission::from_plan(
            &ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                clean_span,
                ObservationRole::Hover,
                origin.clone(),
            )],
        );
        assert!(clean.is_ok(), "{clean:?}");
        let dirty = ProjectionEmission::from_plan(
            &ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 14 },
                ObservationRole::Hover,
                origin,
            )],
        );
        assert!(
            matches!(
                dirty,
                Err(EmissionRefusal::SyntheticAuthoredLocation { .. })
            ),
            "{dirty:?}"
        );
        let empty_origin = ProjectionEmission::from_operation(
            &ct,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 14 },
                ObservationRole::Feature,
                ProjectionOrigin::empty(),
            )],
        );
        assert!(empty_origin.is_ok(), "{empty_origin:?}");
    }

    #[test]
    fn emit_op_lowers_to_mapper_kinds_from_byte_geometry() {
        let allocator = Allocator::default();
        let identity = CodeTransform::new("abcdef", &allocator);
        let unmapped = EmitOp::InsertUnmapped {
            at: SourceByteOffset(0),
            text: EmitText::Static("__VLS_ctx."),
        };
        let mapped = EmitOp::InsertMapped {
            at: SourceByteOffset(0),
            text: EmitText::Borrowed("foo"),
            source_start: SourceByteOffset(0),
            content_offset: GeneratedByteLen(0),
        };
        let preserve = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
        };
        let r#move = EmitOp::MoveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
            at: SourceByteOffset(4),
        };
        let boundary = EmitOp::OverwriteSyntheticBoundary {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(1),
            },
            text: EmitText::Static("{"),
            anchor: None,
        };
        assert_eq!(
            projected_class_for_emit_op(&unmapped, &identity),
            vec![(
                MappingSpan {
                    start: 0,
                    end: "__VLS_ctx.".len() as u32,
                },
                ProjectedClass::Synthesized
            )]
        );
        assert_eq!(
            projected_class_for_emit_op(&mapped, &identity),
            vec![(MappingSpan { start: 0, end: 3 }, ProjectedClass::Rewritten)]
        );
        assert_eq!(
            projected_class_for_emit_op(&preserve, &identity),
            vec![(MappingSpan { start: 0, end: 3 }, ProjectedClass::Identity)]
        );
        assert_eq!(
            projected_class_for_emit_op(&r#move, &identity),
            vec![(MappingSpan { start: 0, end: 3 }, ProjectedClass::Relocated)]
        );
        assert_eq!(
            projected_class_for_emit_op(&boundary, &identity),
            vec![(
                MappingSpan { start: 0, end: 1 },
                ProjectedClass::Synthesized
            )]
        );
        let mixed = EmitOp::InsertMapped {
            at: SourceByteOffset(0),
            text: EmitText::Borrowed("xyfoo"),
            source_start: SourceByteOffset(0),
            content_offset: GeneratedByteLen(2),
        };
        assert_eq!(
            projected_class_for_emit_op(&mixed, &identity),
            vec![
                (
                    MappingSpan { start: 0, end: 2 },
                    ProjectedClass::Synthesized
                ),
                (MappingSpan { start: 2, end: 5 }, ProjectedClass::Rewritten),
            ]
        );
        let wholly_unmapped = EmitOp::InsertMapped {
            at: SourceByteOffset(0),
            text: EmitText::Borrowed("foo"),
            source_start: SourceByteOffset(0),
            content_offset: GeneratedByteLen(3),
        };
        assert_eq!(
            projected_class_for_emit_op(&wholly_unmapped, &identity),
            vec![(
                MappingSpan { start: 0, end: 3 },
                ProjectedClass::Synthesized
            )]
        );
    }

    #[test]
    fn preserve_original_class_follows_mapping_after_adjacent_move() {
        let allocator = Allocator::default();
        let preserve_aa = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(2),
            },
        };
        let mut moved = CodeTransform::new("aabb", &allocator);
        moved.move_slice(2, 4, 0);
        assert_eq!(moved.build_string(), "bbaa");
        let mapping = MappingProduct::of(&moved);
        assert_eq!(
            mapping.projected_at(2).map(|region| region.class),
            Some(ProjectedClass::Relocated),
            "untouched aa at generated 2..4 is Relocated after bb moved ahead"
        );
        assert_eq!(
            projected_class_for_emit_op(&preserve_aa, &moved),
            vec![(MappingSpan { start: 2, end: 4 }, ProjectedClass::Relocated)]
        );
        let control = CodeTransform::new("aabb", &allocator);
        assert_eq!(
            projected_class_for_emit_op(&preserve_aa, &control),
            vec![(MappingSpan { start: 0, end: 2 }, ProjectedClass::Identity)]
        );
    }

    #[test]
    fn preserve_original_class_follows_mapping_after_two_byte_swap() {
        let allocator = Allocator::default();
        let preserve_a = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(1),
            },
        };
        let mut moved = CodeTransform::new("AB", &allocator);
        moved.move_slice(1, 2, 0);
        assert_eq!(moved.build_string(), "BA");
        assert_eq!(
            MappingProduct::of(&moved)
                .projected_at(1)
                .map(|region| region.class),
            Some(ProjectedClass::Relocated)
        );
        assert_eq!(
            projected_class_for_emit_op(&preserve_a, &moved),
            vec![(MappingSpan { start: 1, end: 2 }, ProjectedClass::Relocated)]
        );
        let control = CodeTransform::new("AB", &allocator);
        assert_eq!(
            projected_class_for_emit_op(&preserve_a, &control),
            vec![(MappingSpan { start: 0, end: 1 }, ProjectedClass::Identity)]
        );
    }

    #[test]
    fn preserve_original_omits_partial_rewritten_carrier() {
        let allocator = Allocator::default();
        let mut rewritten = CodeTransform::new("abcdef", &allocator);
        rewritten.overwrite(0, 6, "XY");
        let mapping = MappingProduct::of(&rewritten);
        let region = mapping
            .projected()
            .iter()
            .find(|region| region.class == ProjectedClass::Rewritten)
            .expect("mapped overwrite is rewritten");
        assert_eq!(region.generated, MappingSpan { start: 0, end: 2 });
        assert_eq!(
            region.carrier,
            Some(MappingSpan { start: 0, end: 6 }),
            "generated and carrier lengths differ"
        );
        let preserve_partial = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
        };
        assert_eq!(
            projected_class_for_emit_op(&preserve_partial, &rewritten),
            vec![],
            "partial rewritten overlap must not return the whole generated span"
        );
        let preserve_full = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(6),
            },
        };
        assert_eq!(
            projected_class_for_emit_op(&preserve_full, &rewritten),
            vec![(MappingSpan { start: 0, end: 2 }, ProjectedClass::Rewritten)]
        );
    }

    #[test]
    fn preserve_original_omits_equal_length_rewritten_partial() {
        let allocator = Allocator::default();
        let mut rewritten = CodeTransform::new("abcdef", &allocator);
        rewritten.overwrite(0, 6, "é😃");
        assert_eq!(rewritten.build_string(), "é😃");
        let mapping = MappingProduct::of(&rewritten);
        let region = mapping
            .projected()
            .iter()
            .find(|region| region.class == ProjectedClass::Rewritten)
            .expect("mapped overwrite is rewritten");
        assert_eq!(region.generated, MappingSpan { start: 0, end: 6 });
        assert_eq!(region.carrier, Some(MappingSpan { start: 0, end: 6 }));
        assert_eq!(
            region.generated.len(),
            region.carrier.expect("rewritten carrier").len(),
            "equal lengths must not imply byte-to-byte rewritten correspondence"
        );
        let preserve_partial = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
        };
        assert_eq!(
            projected_class_for_emit_op(&preserve_partial, &rewritten),
            vec![],
            "equal-length rewritten partial must not invent generated [0,3) inside 😃"
        );
        let preserve_full = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(6),
            },
        };
        assert_eq!(
            projected_class_for_emit_op(&preserve_full, &rewritten),
            vec![(MappingSpan { start: 0, end: 6 }, ProjectedClass::Rewritten)]
        );
        let control = CodeTransform::new("abcdef", &allocator);
        assert_eq!(
            projected_class_for_emit_op(&preserve_partial, &control),
            vec![(MappingSpan { start: 0, end: 3 }, ProjectedClass::Identity)],
            "Identity still maps the carrier overlap by offset"
        );
    }

    #[test]
    fn compose_source_chain_uses_code_transform_owner() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("abc", &allocator);
        let upstream = ct.generate_map(
            crate::code_transform::SourceMapOptions::new()
                .with_source("in.js")
                .with_file("out.js"),
        );
        compose_source_chain(&ct, &upstream).expect("identity chain uses CodeTransform");
    }

    #[test]
    fn edit_origin_is_verbatim_only() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("foo", &allocator);
        let origin = ProjectionOrigin::empty();
        let edits = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 3 },
            ObservationRole::Edits,
            origin.clone(),
        );
        let feature = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 3 },
            ObservationRole::Feature,
            origin,
        );
        let emission = ProjectionEmission::from_operation(&ct, vec![edits.clone()]).expect("edits");
        let edit = emission.edit_origin(&edits).expect("verbatim edit");
        assert_eq!(edit.carrier(), MappingSpan { start: 0, end: 3 });
        let feature_only =
            ProjectionEmission::from_operation(&ct, vec![feature.clone()]).expect("feature");
        assert!(
            matches!(
                feature_only.edit_origin(&feature),
                Err(EmissionRefusal::EditNotVerbatim { .. })
            ),
            "feature participation is not edit safety"
        );
        let forged = RoleQualifiedObservation::new(
            feature.generated(),
            ObservationRole::Edits,
            feature.origin().clone(),
        );
        assert!(
            matches!(
                feature_only.edit_origin(&forged),
                Err(EmissionRefusal::EditNotVerbatim { .. })
            ),
            "Feature-only emission rejects a forged Edits observation"
        );
        let stored = ProjectionEmission::from_operation(&ct, vec![edits.clone()]).expect("control");
        assert!(stored.edit_origin(&edits).is_ok(), "stored Edits control");
    }

    fn carrier_span_class_eq(left: &MappingProduct, right: &MappingProduct) -> bool {
        left.carrier_len() == right.carrier_len()
            && left.carrier().len() == right.carrier().len()
            && left
                .carrier()
                .iter()
                .zip(right.carrier())
                .all(|(a, b)| a.source == b.source && a.class == b.class)
    }

    fn helper_preamble_at<'a>(
        allocator: &'a Allocator,
        source: &'a str,
        preamble: &str,
        carrier_anchor: u32,
    ) -> CodeTransform<'a> {
        let mut ct = CodeTransform::new(source, allocator);
        let allocated = ct.alloc_str(preamble);
        ct.batch_prepend_left_static(&[(0, allocated)]);
        ct.set_helper_preamble_content_at(allocated, carrier_anchor);
        ct
    }

    #[test]
    fn stp10_stale_map_rejects_equal_carrier_partition_with_moved_preimage() {
        let allocator = Allocator::default();
        let mut previous_ct = CodeTransform::new("AAAA", &allocator);
        previous_ct.move_slice(0, 1, 0);
        previous_ct.move_slice(0, 1, 0);
        let previous =
            ProjectionEmission::from_operation(&previous_ct, Vec::new()).expect("previous");
        let mut current_ct = CodeTransform::new("AAAA", &allocator);
        current_ct.move_slice(0, 1, 0);
        current_ct.move_slice(0, 1, 4);
        assert_eq!(current_ct.build_string(), previous.checking_text());
        let current = MappingProduct::of(&current_ct);
        assert!(
            carrier_span_class_eq(&current, previous.mapping()),
            "carrier span/class partition stays identical across this move"
        );
        let previous_head = previous
            .mapping()
            .projected_at(0)
            .and_then(|region| region.carrier);
        let current_head = current.projected_at(0).and_then(|region| region.carrier);
        assert_ne!(
            previous_head, current_head,
            "generated 0..1 preimage must move"
        );
        let dirty = ProjectionEmission::reuse_mapping(&previous, &current_ct, None, Vec::new());
        assert!(matches!(dirty, Err(EmissionRefusal::StaleMap)), "{dirty:?}");
        let control_ct = {
            let mut ct = CodeTransform::new("AAAA", &allocator);
            ct.move_slice(0, 1, 0);
            ct.move_slice(0, 1, 0);
            ct
        };
        let control = ProjectionEmission::reuse_mapping(&previous, &control_ct, None, Vec::new());
        assert!(control.is_ok(), "{control:?}");
    }

    #[test]
    fn stp10_correspondence_revision_includes_insertion_anchors() {
        let allocator = Allocator::default();
        let at_zero = helper_preamble_at(&allocator, "abc", "X", 0);
        let at_two = helper_preamble_at(&allocator, "abc", "X", 2);
        assert_eq!(at_zero.build_string(), at_two.build_string());
        let previous = ProjectionEmission::from_operation(&at_zero, Vec::new()).expect("anchor 0");
        let current = ProjectionEmission::from_operation(&at_two, Vec::new()).expect("anchor 2");
        assert_eq!(previous.text_revision(), current.text_revision());
        assert_ne!(
            previous.mapping().insertion_anchors(),
            current.mapping().insertion_anchors()
        );
        assert_eq!(
            previous.mapping().insertion_anchors()[0].carrier,
            0,
            "control keeps the preamble pointer-identity anchor at carrier 0"
        );
        assert_eq!(current.mapping().insertion_anchors()[0].carrier, 2);
        assert_ne!(
            previous.correspondence_revision(),
            current.correspondence_revision()
        );
        let dirty = ProjectionEmission::reuse_mapping(&previous, &at_two, None, Vec::new());
        assert!(matches!(dirty, Err(EmissionRefusal::StaleMap)), "{dirty:?}");
        let control_ct = helper_preamble_at(&allocator, "abc", "X", 0);
        let control = ProjectionEmission::reuse_mapping(&previous, &control_ct, None, Vec::new());
        assert!(control.is_ok(), "{control:?}");
        assert_eq!(
            control.expect("control").correspondence_revision(),
            previous.correspondence_revision()
        );
    }

    #[test]
    fn stp10_edit_origin_rejects_reordered_source_regions() {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("AABB", &allocator);
        ct.move_slice(2, 4, 0);
        assert_eq!(ct.build_string(), "BBAA");
        let origin = ProjectionOrigin::empty();
        let edits = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 4 },
            ObservationRole::Edits,
            origin,
        );
        let emission = ProjectionEmission::from_operation(&ct, vec![edits.clone()]).expect("moved");
        let mapped = emission.edit_origin(&edits);
        assert!(
            matches!(mapped, Err(EmissionRefusal::EditNotVerbatim { .. })),
            "reordered BB+AA cannot collapse to one carrier span: {mapped:?}"
        );
    }

    #[test]
    fn stp10_edit_origin_maps_exact_subspan_and_rejects_discontiguous_preimage() {
        let allocator = Allocator::default();
        let source = "const foo = 1;";
        let ct = CodeTransform::new(source, &allocator);
        let origin = ProjectionOrigin::empty();
        let token = RoleQualifiedObservation::new(
            MappingSpan { start: 6, end: 9 },
            ObservationRole::Edits,
            origin.clone(),
        );
        let emission =
            ProjectionEmission::from_operation(&ct, vec![token.clone()]).expect("identity");
        let edit = emission.edit_origin(&token).expect("token subspan");
        assert_eq!(edit.carrier(), MappingSpan { start: 6, end: 9 });
        assert_eq!(&source[6..9], "foo");

        let mut moved = CodeTransform::new("abcdef", &allocator);
        moved.move_slice(0, 3, 6);
        assert_eq!(moved.build_string(), "defabc");
        let whole = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 6 },
            ObservationRole::Edits,
            origin.clone(),
        );
        let prefix = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 3 },
            ObservationRole::Edits,
            origin,
        );
        let moved_emission =
            ProjectionEmission::from_operation(&moved, vec![prefix.clone()]).expect("relocated");
        let prefix_edit = moved_emission
            .edit_origin(&prefix)
            .expect("exact relocated region");
        assert_eq!(prefix_edit.carrier(), MappingSpan { start: 3, end: 6 });
        let whole_edit = moved_emission.edit_origin(&whole);
        assert!(
            matches!(whole_edit, Err(EmissionRefusal::EditNotVerbatim { .. })),
            "def+abc is a reordered preimage, not carrier [3,3): {whole_edit:?}"
        );
    }

    #[test]
    fn stp10_overlap_rejects_inverted_span_before_adjacent_check() {
        let allocator = Allocator::default();
        let source = "abcdefghij";
        let ct = CodeTransform::new(source, &allocator);
        let origin = ProjectionOrigin::empty();
        let masked = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 10 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 2, end: 0 },
                    ObservationRole::Definition,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 3, end: 4 },
                    ObservationRole::Feature,
                    origin.clone(),
                ),
            ],
        );
        assert!(
            matches!(
                masked,
                Err(EmissionRefusal::ObservationOutsideMapping { .. })
                    | Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "{masked:?}"
        );
        let overlap = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 10 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 3, end: 4 },
                    ObservationRole::Feature,
                    origin.clone(),
                ),
            ],
        );
        assert!(
            matches!(
                overlap,
                Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "removing the reversed interval must still reject the overlap: {overlap:?}"
        );
        let abcdef = CodeTransform::new("abcdef", &allocator);
        let inverted = ProjectionEmission::from_operation(
            &abcdef,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 5, end: 3 },
                ObservationRole::Edits,
                origin.clone(),
            )],
        );
        assert!(
            matches!(
                inverted,
                Err(EmissionRefusal::ObservationOutsideMapping { .. })
            ),
            "{inverted:?}"
        );
        let valid = ProjectionEmission::from_operation(
            &abcdef,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 3, end: 5 },
                ObservationRole::Edits,
                origin,
            )],
        );
        assert!(valid.is_ok(), "{valid:?}");
    }

    #[test]
    fn stp10_insertion_anchor_admits_zero_width_authored_observation() {
        let allocator = Allocator::default();
        let source = ROLE_SFC;
        let plan = role_plan();
        let origin = authored_in(&plan);
        let anchored_ct = helper_preamble_at(&allocator, source, "X", 0);
        let projected = MappingProduct::of(&anchored_ct).insertion_anchors()[0].projected;
        let anchored = RoleQualifiedObservation::new(
            MappingSpan {
                start: projected,
                end: projected,
            },
            ObservationRole::Diagnostic,
            origin.clone(),
        );
        let clean =
            ProjectionEmission::from_plan(&anchored_ct, &plan, ROLE_CANONICAL, vec![anchored]);
        assert!(clean.is_ok(), "{clean:?}");
        let mut unanchored_ct = CodeTransform::new(source, &allocator);
        unanchored_ct.prepend("X");
        let unanchored = ProjectionEmission::from_plan(
            &unanchored_ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 0 },
                ObservationRole::Diagnostic,
                origin.clone(),
            )],
        );
        assert!(
            matches!(
                unanchored,
                Err(EmissionRefusal::SyntheticAuthoredLocation { .. })
            ),
            "{unanchored:?}"
        );
        let nonempty = ProjectionEmission::from_plan(
            &anchored_ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 1 },
                ObservationRole::Diagnostic,
                origin,
            )],
        );
        assert!(
            matches!(
                nonempty,
                Err(EmissionRefusal::SyntheticAuthoredLocation { .. })
            ),
            "{nonempty:?}"
        );
    }

    #[test]
    fn stp10_edit_origin_rejects_mid_utf8_span() {
        let allocator = Allocator::default();
        let source = "é";
        let ct = CodeTransform::new(source, &allocator);
        let origin = ProjectionOrigin::empty();
        let split = RoleQualifiedObservation::new(
            MappingSpan { start: 1, end: 2 },
            ObservationRole::Edits,
            origin.clone(),
        );
        let whole = RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 2 },
            ObservationRole::Edits,
            origin.clone(),
        );
        let whole_emission =
            ProjectionEmission::from_operation(&ct, vec![whole.clone()]).expect("whole character");
        assert_eq!(
            whole_emission
                .edit_origin(&whole)
                .expect("whole-character edit")
                .carrier(),
            MappingSpan { start: 0, end: 2 }
        );
        let split_emission = ProjectionEmission::from_operation(&ct, vec![split.clone()]);
        assert!(
            matches!(
                split_emission,
                Err(EmissionRefusal::ObservationNotOnCharBoundary { .. })
            ),
            "mid-UTF-8 [1,2) of é must not assemble: {split_emission:?}"
        );
        let hover_split = ProjectionEmission::from_operation(
            &ct,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 1, end: 2 },
                ObservationRole::Hover,
                origin.clone(),
            )],
        );
        assert!(
            matches!(
                hover_split,
                Err(EmissionRefusal::ObservationNotOnCharBoundary { .. })
            ),
            "Hover [1,2) inside é is not a character-boundary source target: {hover_split:?}"
        );
        let hover_whole = ProjectionEmission::from_operation(
            &ct,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 2 },
                ObservationRole::Hover,
                origin,
            )],
        );
        assert!(hover_whole.is_ok(), "{hover_whole:?}");
    }

    #[test]
    fn stp10_origin_requires_emission_snapshot() {
        let allocator = Allocator::default();
        let plan = role_plan();
        let origin = authored_in(&plan);
        let ct = CodeTransform::new(ROLE_SFC, &allocator);
        let span = vfor_alias_span(ROLE_SFC);
        let hover = RoleQualifiedObservation::new(span, ObservationRole::Hover, origin.clone());
        let same = ProjectionEmission::from_plan(&ct, &plan, ROLE_CANONICAL, vec![hover.clone()]);
        assert!(same.is_ok(), "{same:?}");
        assert_eq!(same.expect("bound").snapshot(), Some(&plan.snapshot));

        let other_plan = plan_from_source("file:///stp10-other.vue", ROLE_SFC);
        let other_origin = authored_in(&other_plan);
        assert_ne!(origin.snapshot(), other_origin.snapshot());
        let other_file = ProjectionEmission::from_plan(
            &ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                span,
                ObservationRole::Hover,
                other_origin,
            )],
        );
        assert!(
            matches!(other_file, Err(EmissionRefusal::UnboundOrigin)),
            "origin from another canonical file: {other_file:?}"
        );

        let mutated = format!("{ROLE_SFC} ");
        let mutated_plan = plan_from_source(ROLE_CANONICAL, &mutated);
        assert_ne!(plan.snapshot, mutated_plan.snapshot);
        let mutated_origin = authored_in(&mutated_plan);
        let other_snapshot = ProjectionEmission::from_plan(
            &ct,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                span,
                ObservationRole::Hover,
                mutated_origin,
            )],
        );
        assert!(
            matches!(other_snapshot, Err(EmissionRefusal::UnboundOrigin)),
            "origin from another snapshot: {other_snapshot:?}"
        );

        let foo = CodeTransform::new("foo", &allocator);
        let cross_file = ProjectionEmission::from_operation(
            &foo,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 3 },
                ObservationRole::Hover,
                origin.clone(),
            )],
        );
        assert!(
            matches!(cross_file, Err(EmissionRefusal::UnboundOrigin)),
            "authored origin without the emission plan: {cross_file:?}"
        );
        let mismatched_carrier = ProjectionEmission::from_plan(
            &foo,
            &plan,
            ROLE_CANONICAL,
            vec![RoleQualifiedObservation::new(
                MappingSpan { start: 0, end: 3 },
                ObservationRole::Hover,
                origin,
            )],
        );
        assert!(
            matches!(mismatched_carrier, Err(EmissionRefusal::UnboundOrigin)),
            "plan snapshot does not match transform original: {mismatched_carrier:?}"
        );
    }

    #[test]
    fn stp10_reuse_mapping_preserves_plan_binding() {
        let allocator = Allocator::default();
        let plan = role_plan();
        let origin = authored_in(&plan);
        let ct = CodeTransform::new(ROLE_SFC, &allocator);
        let span = vfor_alias_span(ROLE_SFC);
        let observations = vec![RoleQualifiedObservation::new(
            span,
            ObservationRole::Hover,
            origin,
        )];
        let previous =
            ProjectionEmission::from_plan(&ct, &plan, ROLE_CANONICAL, observations.clone())
                .expect("plan-bound emission");
        assert_eq!(previous.snapshot(), Some(&plan.snapshot));
        let missing_current =
            ProjectionEmission::reuse_mapping(&previous, &ct, None, observations.clone());
        assert!(
            matches!(missing_current, Err(EmissionRefusal::StaleMap)),
            "bound reuse without the current plan is stale lineage: {missing_current:?}"
        );
        let reused = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan, ROLE_CANONICAL)),
            observations.clone(),
        )
        .expect("reuse");
        assert_eq!(reused.snapshot(), previous.snapshot());
        assert_eq!(reused.input_basis(), previous.input_basis());
        assert_eq!(
            reused.correspondence_revision(),
            previous.correspondence_revision()
        );

        let unbound = ProjectionEmission::from_operation(&ct, Vec::new()).expect("unbound");
        assert!(unbound.snapshot().is_none());
        let dirty = ProjectionEmission::reuse_mapping(&unbound, &ct, None, observations);
        assert!(
            matches!(dirty, Err(EmissionRefusal::UnboundOrigin)),
            "reuse of an unbound emission cannot admit authored origins: {dirty:?}"
        );
    }

    #[test]
    fn stp10_reuse_mapping_rejects_equal_geometry_source_edit() {
        let allocator = Allocator::default();
        let source_a = format!("<!--a-->{ROLE_SFC}");
        let source_b = format!("<!--b-->{ROLE_SFC}");
        let prefix = "<!--a-->".len() as u32;
        let plan = plan_from_source(ROLE_CANONICAL, &source_a);
        let origin = authored_in(&plan);
        let mut previous_ct = CodeTransform::new(&source_a, &allocator);
        previous_ct.overwrite_unmapped(0, prefix, "");
        let mut current_ct = CodeTransform::new(&source_b, &allocator);
        current_ct.overwrite_unmapped(0, prefix, "");
        assert_eq!(previous_ct.build_string(), current_ct.build_string());
        assert_eq!(
            MappingProduct::of(&previous_ct),
            MappingProduct::of(&current_ct)
        );
        let observations = vec![RoleQualifiedObservation::new(
            vfor_alias_span(ROLE_SFC),
            ObservationRole::Edits,
            origin,
        )];
        let previous = ProjectionEmission::from_plan(
            &previous_ct,
            &plan,
            ROLE_CANONICAL,
            observations.clone(),
        )
        .expect("elided comment binds");
        assert!(previous.edit_origin(&observations[0]).is_ok());
        let dirty = ProjectionEmission::reuse_mapping(
            &previous,
            &current_ct,
            Some((&plan, ROLE_CANONICAL)),
            observations.clone(),
        );
        assert!(matches!(dirty, Err(EmissionRefusal::StaleMap)), "{dirty:?}");
        let plan_b = plan_from_source(ROLE_CANONICAL, &source_b);
        assert_ne!(plan.snapshot, plan_b.snapshot);
        let fresh = ProjectionEmission::from_plan(
            &current_ct,
            &plan_b,
            ROLE_CANONICAL,
            observations.clone(),
        );
        assert!(
            matches!(fresh, Err(EmissionRefusal::UnboundOrigin)),
            "stale observation snapshot cannot bind the new plan: {fresh:?}"
        );
        let control_ct = {
            let mut ct = CodeTransform::new(&source_a, &allocator);
            ct.overwrite_unmapped(0, prefix, "");
            ct
        };
        let control = ProjectionEmission::reuse_mapping(
            &previous,
            &control_ct,
            Some((&plan, ROLE_CANONICAL)),
            observations,
        )
        .expect("unchanged source");
        assert_eq!(control.snapshot(), previous.snapshot());
        assert_eq!(
            control.correspondence_revision(),
            previous.correspondence_revision()
        );
    }

    struct ParseKeyMarker(&'static str);

    impl CanonicalEncode for ParseKeyMarker {
        const DOMAIN_TAG: &'static str = "verter.test.stp10.parse_key_marker";
        fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
            encoder.field_str(1, self.0);
        }
    }

    fn plan_from_source_with_parse(
        canonical_id: &str,
        source: &str,
        parse_key: Option<&ParseKey>,
    ) -> ProjectionPlan {
        let parsed = crate::compile::parse_sfc(source, None, None);
        build_projection_plan(PlanInput {
            canonical_id,
            source,
            parsed: &parsed,
            parse_key,
            syntax_profile: None,
        })
    }

    fn hover_on_alias(plan: &ProjectionPlan, source: &str) -> Vec<RoleQualifiedObservation> {
        vec![RoleQualifiedObservation::new(
            vfor_alias_span(source),
            ObservationRole::Hover,
            authored_in(plan),
        )]
    }

    #[test]
    fn stp10_reuse_mapping_rejects_cross_canonical_same_bytes() {
        let allocator = Allocator::default();
        let plan_a = plan_from_source("file:///a.vue", ROLE_SFC);
        let plan_b = plan_from_source("file:///b.vue", ROLE_SFC);
        assert_ne!(
            plan_a.snapshot, plan_b.snapshot,
            "identical bytes under different canonicals must mint distinct snapshots"
        );
        let ct = CodeTransform::new(ROLE_SFC, &allocator);
        let observations = hover_on_alias(&plan_a, ROLE_SFC);
        let previous =
            ProjectionEmission::from_plan(&ct, &plan_a, "file:///a.vue", observations.clone())
                .expect("a.vue binds");
        let dirty = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_b, "file:///b.vue")),
            observations.clone(),
        );
        assert!(
            matches!(dirty, Err(EmissionRefusal::StaleMap)),
            "reuse must not keep a.vue under b.vue: {dirty:?}"
        );
        let from_b =
            ProjectionEmission::from_plan(&ct, &plan_b, "file:///b.vue", observations.clone());
        assert!(
            matches!(from_b, Err(EmissionRefusal::UnboundOrigin)),
            "a.vue origin cannot bind b.vue: {from_b:?}"
        );
        let control = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_a, "file:///a.vue")),
            observations,
        )
        .expect("same canonical reuses");
        assert_eq!(control.snapshot(), previous.snapshot());
    }

    #[test]
    fn stp10_reuse_mapping_rejects_parse_key_twin() {
        let allocator = Allocator::default();
        let key_a = ParseKey::from_canonical(&ParseKeyMarker("a"));
        let key_b = ParseKey::from_canonical(&ParseKeyMarker("b"));
        let plan_a = plan_from_source_with_parse(ROLE_CANONICAL, ROLE_SFC, Some(&key_a));
        let plan_b = plan_from_source_with_parse(ROLE_CANONICAL, ROLE_SFC, Some(&key_b));
        assert_ne!(
            plan_a.snapshot, plan_b.snapshot,
            "same source and canonical with a different parse key must mint distinct snapshots"
        );
        let ct = CodeTransform::new(ROLE_SFC, &allocator);
        let observations = hover_on_alias(&plan_a, ROLE_SFC);
        let previous =
            ProjectionEmission::from_plan(&ct, &plan_a, ROLE_CANONICAL, observations.clone())
                .expect("parse-key A binds");
        let dirty = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_b, ROLE_CANONICAL)),
            observations.clone(),
        );
        assert!(
            matches!(dirty, Err(EmissionRefusal::StaleMap)),
            "reuse must not keep parse-key A under parse-key B: {dirty:?}"
        );
        let from_b =
            ProjectionEmission::from_plan(&ct, &plan_b, ROLE_CANONICAL, observations.clone());
        assert!(
            matches!(from_b, Err(EmissionRefusal::UnboundOrigin)),
            "parse-key A origin cannot bind parse-key B: {from_b:?}"
        );
        let control = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_a, ROLE_CANONICAL)),
            observations,
        )
        .expect("same parse key reuses");
        assert_eq!(control.snapshot(), previous.snapshot());
    }

    const FOREIGN_SFC: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const rows = [1];\n",
        "</script>\n",
        "<template>\n",
        "  <div v-for=\"row in rows\">{{ row }}</div>\n",
        "</template>\n",
    );

    #[test]
    fn stp10_reuse_mapping_rejects_foreign_plan_origin_id() {
        let allocator = Allocator::default();
        let plan_a = role_plan();
        let plan_b = plan_from_source("file:///stp10-foreign.vue", FOREIGN_SFC);
        let foreign_row = plan_b
            .origins
            .iter()
            .find(|origin| origin.name == "row")
            .cloned()
            .expect("v-for row");
        assert_ne!(
            binding_item_in(&plan_a),
            foreign_row.id,
            "plan B's binder is not a member of plan A"
        );
        let mut cloned = plan_a.clone();
        cloned.origins.push(foreign_row.clone());
        let foreign = ProjectionOrigin::bind(&cloned, Some(foreign_row.id.clone()), None, None)
            .expect("cloned plan A admits the foreign id");
        assert_eq!(foreign.snapshot(), Some(&plan_a.snapshot));
        let ct = CodeTransform::new(ROLE_SFC, &allocator);
        let span = vfor_alias_span(ROLE_SFC);
        let foreign_obs = vec![RoleQualifiedObservation::new(
            span,
            ObservationRole::Hover,
            foreign,
        )];
        let fresh =
            ProjectionEmission::from_plan(&ct, &plan_a, ROLE_CANONICAL, foreign_obs.clone());
        assert!(
            matches!(fresh, Err(EmissionRefusal::UnboundOrigin)),
            "fresh from_plan must refuse a plan-B id: {fresh:?}"
        );
        let previous = ProjectionEmission::from_plan(
            &ct,
            &plan_a,
            ROLE_CANONICAL,
            hover_on_alias(&plan_a, ROLE_SFC),
        )
        .expect("plan A binds");
        let reused = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_a, ROLE_CANONICAL)),
            foreign_obs,
        );
        assert!(
            matches!(reused, Err(EmissionRefusal::UnboundOrigin)),
            "reuse must not admit a plan-B id under plan A's snapshot: {reused:?}"
        );
        let control = ProjectionEmission::reuse_mapping(
            &previous,
            &ct,
            Some((&plan_a, ROLE_CANONICAL)),
            hover_on_alias(&plan_a, ROLE_SFC),
        )
        .expect("same plan A id reuses");
        assert_eq!(control.snapshot(), previous.snapshot());
    }

    #[test]
    fn stp10_overlap_sweep_compares_linearly() {
        let allocator = Allocator::default();
        let n = 512u32;
        let source = "a".repeat(n as usize);
        let ct = CodeTransform::new(&source, &allocator);
        let origin = ProjectionOrigin::empty();
        let disjoint: Vec<_> = (0..n)
            .map(|i| {
                RoleQualifiedObservation::new(
                    MappingSpan {
                        start: i,
                        end: i + 1,
                    },
                    ObservationRole::Hover,
                    origin.clone(),
                )
            })
            .collect();
        let mut sorted = disjoint.clone();
        sorted.sort_by_key(|obs| (obs.generated.start, obs.generated.end, obs.role.tag()));
        let (overlap, work) = overlapping_virtual_spans_with_work(&sorted);
        assert!(overlap.is_none(), "{overlap:?}");
        assert_eq!(work, (n as usize).saturating_sub(1));
        let quadratic = (n as usize).saturating_mul((n as usize).saturating_sub(1)) / 2;
        assert!(
            work < quadratic,
            "sweep work {work} must stay below the n(n-1)/2 pair walk {quadratic}"
        );
        let clean = ProjectionEmission::from_operation(&ct, disjoint);
        assert!(clean.is_ok(), "{clean:?}");

        let shared = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 1 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 1 },
                    ObservationRole::Edits,
                    origin.clone(),
                ),
            ],
        );
        assert!(
            matches!(shared, Err(EmissionRefusal::OverlappingVirtualSpans { .. })),
            "Hover and Edits on [0,1) overlap: {shared:?}"
        );

        let nested_cover = ProjectionEmission::from_operation(
            &ct,
            vec![
                RoleQualifiedObservation::new(
                    MappingSpan { start: 0, end: 8 },
                    ObservationRole::Hover,
                    origin.clone(),
                ),
                RoleQualifiedObservation::new(
                    MappingSpan { start: 3, end: 4 },
                    ObservationRole::Definition,
                    origin,
                ),
            ],
        );
        assert!(
            matches!(
                nested_cover,
                Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "covering non-edit intervals still overlap: {nested_cover:?}"
        );
    }

    #[test]
    fn stp10_overlap_rejects_point_observations_at_same_point() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("abcdef", &allocator);
        let origin = ProjectionOrigin::empty();

        // Two points at the exact same point [2, 2)
        let same_points = vec![
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 2 },
                ObservationRole::Hover,
                origin.clone(),
            ),
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 2 },
                ObservationRole::Definition,
                origin.clone(),
            ),
        ];
        let rejected = ProjectionEmission::from_operation(&ct, same_points);
        assert!(
            matches!(
                rejected,
                Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "two points at identical offset must overlap: {rejected:?}"
        );

        // Point at start of non-empty interval [2, 2) and [2, 5)
        let point_at_start = vec![
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 2 },
                ObservationRole::Hover,
                origin.clone(),
            ),
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 5 },
                ObservationRole::Definition,
                origin.clone(),
            ),
        ];
        let rejected_start = ProjectionEmission::from_operation(&ct, point_at_start);
        assert!(
            matches!(
                rejected_start,
                Err(EmissionRefusal::OverlappingVirtualSpans { .. })
            ),
            "point at start of range must overlap: {rejected_start:?}"
        );

        // Point at end of non-empty interval [5, 5) and [2, 5) is non-overlapping (half-open)
        let point_at_end = vec![
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 5 },
                ObservationRole::Hover,
                origin.clone(),
            ),
            RoleQualifiedObservation::new(
                MappingSpan { start: 5, end: 5 },
                ObservationRole::Definition,
                origin.clone(),
            ),
        ];
        let control_end = ProjectionEmission::from_operation(&ct, point_at_end);
        assert!(
            control_end.is_ok(),
            "point at end boundary must not overlap: {control_end:?}"
        );

        // Distinct points [2, 2) and [4, 4) do not overlap
        let distinct_points = vec![
            RoleQualifiedObservation::new(
                MappingSpan { start: 2, end: 2 },
                ObservationRole::Hover,
                origin.clone(),
            ),
            RoleQualifiedObservation::new(
                MappingSpan { start: 4, end: 4 },
                ObservationRole::Definition,
                origin,
            ),
        ];
        let control_points = ProjectionEmission::from_operation(&ct, distinct_points);
        assert!(
            control_points.is_ok(),
            "distinct points must not overlap: {control_points:?}"
        );
    }

    #[test]
    fn move_original_class_follows_mapping_with_overwritten_replacement() {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("abcdef", &allocator);
        ct.overwrite(0, 3, "XY");
        ct.move_slice(0, 3, 6);
        assert_eq!(ct.build_string(), "defXY");

        let r#move = EmitOp::MoveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
            at: SourceByteOffset(6),
        };
        assert_eq!(
            projected_class_for_emit_op(&r#move, &ct),
            vec![(MappingSpan { start: 0, end: 2 }, ProjectedClass::Rewritten)],
            "moved overwritten content must report Rewritten with replacement length"
        );

        let mut control = CodeTransform::new("abcdef", &allocator);
        control.move_slice(0, 3, 6);
        assert_eq!(control.build_string(), "defabc");
        assert_eq!(
            projected_class_for_emit_op(&r#move, &control),
            vec![(MappingSpan { start: 0, end: 3 }, ProjectedClass::Relocated)],
            "untouched moved content must report Relocated with original length"
        );
    }

    #[test]
    fn stp10_reuse_mapping_unbound_rejects_mismatched_plan_source_or_canonical() {
        let allocator = Allocator::default();
        let ct_foo = CodeTransform::new("foo", &allocator);
        let unbound = ProjectionEmission::from_operation(&ct_foo, Vec::new()).expect("unbound");
        assert!(unbound.snapshot().is_none());

        let foreign_plan = plan_from_source("file:///different.vue", FOREIGN_SFC);
        // Supplying a plan for different source must be rejected as StaleMap
        let mismatched_source = ProjectionEmission::reuse_mapping(
            &unbound,
            &ct_foo,
            Some((&foreign_plan, "file:///different.vue")),
            Vec::new(),
        );
        assert!(
            matches!(mismatched_source, Err(EmissionRefusal::StaleMap)),
            "reuse with mismatched plan source must fail: {mismatched_source:?}"
        );

        let plan_foo = plan_from_source("file:///foo.vue", "foo");
        let mismatched_canonical = ProjectionEmission::reuse_mapping(
            &unbound,
            &ct_foo,
            Some((&plan_foo, "file:///other.vue")),
            Vec::new(),
        );
        assert!(
            matches!(mismatched_canonical, Err(EmissionRefusal::StaleMap)),
            "reuse with mismatched canonical must fail: {mismatched_canonical:?}"
        );

        let control = ProjectionEmission::reuse_mapping(
            &unbound,
            &ct_foo,
            Some((&plan_foo, "file:///foo.vue")),
            Vec::new(),
        );
        assert!(control.is_ok(), "matching plan succeeds: {control:?}");
    }

    #[test]
    fn roundtrip_verbatim_refuses_unslicable_or_out_of_bounds_spans() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("hello", &allocator);
        let emission = ProjectionEmission::from_operation(&ct, Vec::new()).expect("emission");

        // Normal roundtrip matches "hello"
        assert!(emission.roundtrip_verbatim("hello").is_ok());

        // Source too short: carrier [0, 5) exceeds source length 3
        let short_source = emission.roundtrip_verbatim("hel");
        assert!(
            matches!(short_source, Err(EmissionRefusal::RoundtripMismatch { .. })),
            "short source carrier out of bounds must fail: {short_source:?}"
        );

        // Source with different byte length on multi-byte emoji
        let emoji_ct = CodeTransform::new("🦀", &allocator);
        let emoji_emission =
            ProjectionEmission::from_operation(&emoji_ct, Vec::new()).expect("emission");
        let bad_boundary = emoji_emission.roundtrip_verbatim("\u{FFFD}");
        assert!(
            matches!(bad_boundary, Err(EmissionRefusal::RoundtripMismatch { .. })),
            "invalid boundary must fail: {bad_boundary:?}"
        );
    }

    #[test]
    fn projected_class_for_emit_op_with_matches_convenience_wrapper() {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("abcdef", &allocator);
        ct.overwrite(0, 3, "XY");
        ct.move_slice(0, 3, 6);
        let mapping = MappingProduct::of(&ct);

        let r#move = EmitOp::MoveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(0),
                end: SourceByteOffset(3),
            },
            at: SourceByteOffset(6),
        };
        let preserve = EmitOp::PreserveOriginal {
            source: SourceByteRange {
                start: SourceByteOffset(3),
                end: SourceByteOffset(6),
            },
        };

        assert_eq!(
            projected_class_for_emit_op_with(&r#move, &mapping),
            projected_class_for_emit_op(&r#move, &ct)
        );
        assert_eq!(
            projected_class_for_emit_op_with(&preserve, &mapping),
            projected_class_for_emit_op(&preserve, &ct)
        );
    }

    #[test]
    fn unbound_reuse_mapping_with_none_binding_rejects_authored_observations() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("hello", &allocator);
        let unbound = ProjectionEmission::from_operation(&ct, Vec::new()).expect("unbound");
        assert!(unbound.snapshot().is_none());

        // Reusing without current plan succeeds for empty observations
        let reused_empty = ProjectionEmission::reuse_mapping(&unbound, &ct, None, Vec::new());
        assert!(reused_empty.is_ok());
        let reused = reused_empty.unwrap();
        assert!(reused.snapshot().is_none());

        // But refusing authored observations because no plan is bound
        let plan = role_plan();
        let expression = plan.expressions().iter().next().expect("expression");
        let authored_origin =
            ProjectionOrigin::bind(&plan, None, None, Some(expression.id.clone()))
                .expect("bound origin");
        let authored = vec![RoleQualifiedObservation::new(
            MappingSpan { start: 0, end: 5 },
            ObservationRole::Hover,
            authored_origin,
        )];
        let rejected = ProjectionEmission::reuse_mapping(&unbound, &ct, None, authored);
        assert!(
            matches!(rejected, Err(EmissionRefusal::UnboundOrigin)),
            "unbound reuse must refuse authored origin: {rejected:?}"
        );
    }
}
