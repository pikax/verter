//! Compiler-owned `CustomBlockDescriptor`: opaque source-backed
//! attachments with validated identity, content state, and staged relations.
//! The consumer authority for custom-block facts on the neutral bundle and
//! in session publication. Construction is metadata-only. No producer,
//! transform, plugin ABI, source load, or framework branch lives here.

use std::collections::{BTreeMap, BTreeSet};

use verter_identity::identity::{ContentId, SourceId, SourceRevision, SourceUnitId};
use verter_span::Span;

use super::fragment::{ArtifactId, ArtifactProvenance, ArtifactUnavailableReason, CompileArtifact};
use super::source_unit::ArtifactSourceUnit;

/// Canonical descriptor lineage. Bound fields include source unit, source
/// incarnation, revision, role, order, region, attributes, lang/src, and
/// content state — unlike [`ArtifactId`], a source edit remints this id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomBlockDescriptorId(verter_identity::canonical::Canonical);

impl CustomBlockDescriptorId {
    #[allow(clippy::too_many_arguments)]
    fn mint(
        source_unit: &SourceUnitId,
        source_id: &SourceId,
        revision: &SourceRevision,
        source_content: &ContentId,
        role: &str,
        lang: Option<&str>,
        src: Option<&str>,
        attributes: &[(String, String)],
        source_order: u32,
        region: Span,
        content: &CustomBlockContent,
    ) -> Self {
        let mut e = verter_identity::encoding::CanonicalEncoder::new(
            "verter.compiler.custom_block.descriptor.v1",
        );
        e.field_bytes(1, source_unit.canonical_bytes());
        e.field_bytes(2, source_id.canonical_bytes());
        e.field_bytes(3, revision.canonical_bytes());
        e.field_bytes(4, source_content.canonical_bytes());
        e.field_str(5, role);
        e.field_u32(6, source_order);
        e.field_u32(7, region.start);
        e.field_u32(8, region.end);
        e.field_option(9, lang.map(str::as_bytes));
        e.field_option(10, src.map(str::as_bytes));
        e.field_bytes(11, &encode_attributes(attributes));
        e.field_bytes(12, &encode_content_state(content));
        Self(verter_identity::canonical::Canonical::from_encoder(&e))
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        self.0.bytes()
    }
}

/// Publication/warm eligibility. Only [`Self::Complete`] may attach or warm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomBlockLifecycle {
    Complete,
    Partial,
    Cancelled,
    Stale,
}

/// Opaque block body. External/unavailable bytes are never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomBlockContent {
    Local { content: ContentId, text: String },
    SrcBacked,
    Empty,
    Unavailable(ArtifactUnavailableReason),
}

/// Untrusted construction input. [`CustomBlockDescriptor::try_new`] is the
/// only mint site.
#[derive(Debug, Clone)]
pub struct CustomBlockDescriptorRequest {
    pub source_unit: SourceUnitId,
    /// Source incarnation (`SourceId`), distinct from unit lineage and revision.
    pub source_id: SourceId,
    pub revision: SourceRevision,
    pub source_content: ContentId,
    pub role: String,
    pub lang: Option<String>,
    pub src: Option<String>,
    pub attributes: Vec<(String, String)>,
    /// Position of this block within its [`SourceId`]. Unique per `SourceId`
    /// in an attached set. After the per-source sort, `region.start` must
    /// strictly increase with `source_order`.
    pub source_order: u32,
    /// SFC-absolute content region in UTF-8 bytes, for every content state.
    /// For [`CustomBlockContent::Local`], `region.len()` must equal `text.len()`.
    /// [`CustomBlockContent::Empty`], [`CustomBlockContent::SrcBacked`], and
    /// [`CustomBlockContent::Unavailable`] do not constrain `region.len()`;
    /// the span must still lie inside the staged source-unit span.
    pub region: Span,
    pub content: CustomBlockContent,
    pub provenance: ArtifactProvenance,
    pub attached_to: ArtifactId,
    pub lifecycle: CustomBlockLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomBlockDescriptorError {
    Malformed,
    AliasedAttribute,
    InvalidRegion,
    InvalidOrder,
    Stale,
    Cancelled,
    Partial,
    SourceMismatch,
    DuplicateOrder,
    DuplicateIdentity,
    UnknownSourceUnit,
    UnknownRelationTarget,
    MissingPrimaryInput,
}

impl std::fmt::Display for CustomBlockDescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid custom-block descriptor: {self:?}")
    }
}
impl std::error::Error for CustomBlockDescriptorError {}

/// Immutable source-backed custom-block descriptor. Inspection only after
/// construction; no transform semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomBlockDescriptor {
    id: CustomBlockDescriptorId,
    source_unit: SourceUnitId,
    source_id: SourceId,
    revision: SourceRevision,
    source_content: ContentId,
    role: String,
    lang: Option<String>,
    src: Option<String>,
    attributes: Vec<(String, String)>,
    source_order: u32,
    region: Span,
    content: CustomBlockContent,
    provenance: ArtifactProvenance,
    attached_to: ArtifactId,
    lifecycle: CustomBlockLifecycle,
}

impl CustomBlockDescriptor {
    /// Bounded metadata validation. Does not parse, load, transform, or
    /// evaluate block content.
    pub fn try_new(
        request: CustomBlockDescriptorRequest,
    ) -> Result<Self, CustomBlockDescriptorError> {
        use CustomBlockDescriptorError as E;
        if request.role.is_empty() || request.region.start > request.region.end {
            return Err(E::Malformed);
        }
        if request.lang.as_ref().is_some_and(|value| value.is_empty())
            || request.src.as_ref().is_some_and(|value| value.is_empty())
        {
            return Err(E::Malformed);
        }
        let mut seen = BTreeSet::new();
        for (key, _) in &request.attributes {
            if key.is_empty() || !seen.insert(key.as_str()) {
                return Err(if key.is_empty() {
                    E::Malformed
                } else {
                    E::AliasedAttribute
                });
            }
        }
        check_named_attr(&request.attributes, "lang", request.lang.as_deref())?;
        check_named_attr(&request.attributes, "src", request.src.as_deref())?;
        match (&request.content, request.src.as_deref()) {
            (CustomBlockContent::Local { .. }, Some(_)) | (CustomBlockContent::Empty, Some(_)) => {
                return Err(E::Malformed)
            }
            (CustomBlockContent::SrcBacked, None) => return Err(E::Malformed),
            (CustomBlockContent::Local { text, content }, None) => {
                if text.is_empty() {
                    return Err(E::Malformed);
                }
                if u32::try_from(text.len()).ok() != Some(request.region.len()) {
                    return Err(E::InvalidRegion);
                }
                if *content != ContentId::from_content_bytes(text.as_bytes()) {
                    return Err(E::Malformed);
                }
            }
            (CustomBlockContent::Empty, None)
            | (CustomBlockContent::SrcBacked, Some(_))
            | (CustomBlockContent::Unavailable(_), _) => {}
        }
        if !request.provenance.inputs.contains(&request.source_unit) {
            return Err(E::MissingPrimaryInput);
        }
        let id = CustomBlockDescriptorId::mint(
            &request.source_unit,
            &request.source_id,
            &request.revision,
            &request.source_content,
            &request.role,
            request.lang.as_deref(),
            request.src.as_deref(),
            &request.attributes,
            request.source_order,
            request.region,
            &request.content,
        );
        Ok(Self {
            id,
            source_unit: request.source_unit,
            source_id: request.source_id,
            revision: request.revision,
            source_content: request.source_content,
            role: request.role,
            lang: request.lang,
            src: request.src,
            attributes: request.attributes,
            source_order: request.source_order,
            region: request.region,
            content: request.content,
            provenance: request.provenance,
            attached_to: request.attached_to,
            lifecycle: request.lifecycle,
        })
    }

    pub fn id(&self) -> &CustomBlockDescriptorId {
        &self.id
    }
    pub fn source_unit(&self) -> &SourceUnitId {
        &self.source_unit
    }
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }
    pub fn revision(&self) -> &SourceRevision {
        &self.revision
    }
    pub fn source_content(&self) -> &ContentId {
        &self.source_content
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }
    pub fn src(&self) -> Option<&str> {
        self.src.as_deref()
    }
    pub fn attributes(&self) -> &[(String, String)] {
        &self.attributes
    }
    pub fn source_order(&self) -> u32 {
        self.source_order
    }
    pub fn region(&self) -> Span {
        self.region
    }
    pub fn content(&self) -> &CustomBlockContent {
        &self.content
    }
    pub fn provenance(&self) -> &ArtifactProvenance {
        &self.provenance
    }
    pub fn attached_to(&self) -> &ArtifactId {
        &self.attached_to
    }
    pub fn lifecycle(&self) -> CustomBlockLifecycle {
        self.lifecycle
    }
}

fn check_named_attr(
    attributes: &[(String, String)],
    name: &str,
    dedicated: Option<&str>,
) -> Result<(), CustomBlockDescriptorError> {
    let from_attrs = attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str());
    if dedicated != from_attrs {
        return Err(CustomBlockDescriptorError::AliasedAttribute);
    }
    Ok(())
}

fn encode_attributes(attributes: &[(String, String)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(attributes.len() as u64).to_le_bytes());
    for (key, value) in attributes {
        payload.extend_from_slice(&(key.len() as u64).to_le_bytes());
        payload.extend_from_slice(key.as_bytes());
        payload.extend_from_slice(&(value.len() as u64).to_le_bytes());
        payload.extend_from_slice(value.as_bytes());
    }
    payload
}

fn encode_content_state(content: &CustomBlockContent) -> Vec<u8> {
    match content {
        CustomBlockContent::Local { content, .. } => {
            let mut payload = vec![0u8];
            payload.extend_from_slice(content.canonical_bytes());
            payload
        }
        CustomBlockContent::SrcBacked => vec![1],
        CustomBlockContent::Empty => vec![2],
        CustomBlockContent::Unavailable(reason) => {
            let tag = match reason {
                ArtifactUnavailableReason::Unsupported => "unsupported",
                ArtifactUnavailableReason::InvalidInput => "invalidInput",
                ArtifactUnavailableReason::NotProduced => "notProduced",
            };
            let mut payload = vec![3u8];
            payload.extend_from_slice(tag.as_bytes());
            payload
        }
    }
}

pub(crate) fn validate_attachment(
    source_units: &BTreeMap<SourceUnitId, ArtifactSourceUnit>,
    artifacts: &[CompileArtifact],
    mut descriptors: Vec<CustomBlockDescriptor>,
) -> Result<Vec<CustomBlockDescriptor>, CustomBlockDescriptorError> {
    use CustomBlockDescriptorError as E;
    descriptors.sort_by(|a, b| {
        a.source_id
            .cmp(&b.source_id)
            .then(a.source_order.cmp(&b.source_order))
            .then(a.id.cmp(&b.id))
    });
    let mut orders: BTreeMap<&SourceId, BTreeSet<u32>> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut regions: BTreeMap<&SourceId, Vec<Span>> = BTreeMap::new();
    for descriptor in &descriptors {
        check_against_set(source_units, artifacts, descriptor)?;
        if !ids.insert(&descriptor.id) {
            return Err(E::DuplicateIdentity);
        }
        if !orders
            .entry(&descriptor.source_id)
            .or_default()
            .insert(descriptor.source_order)
        {
            return Err(E::DuplicateOrder);
        }
        let source_regions = regions.entry(&descriptor.source_id).or_default();
        if source_regions
            .iter()
            .any(|existing| spans_overlap(*existing, descriptor.region))
        {
            return Err(E::InvalidRegion);
        }
        if source_regions
            .last()
            .is_some_and(|previous| descriptor.region.start <= previous.start)
        {
            return Err(E::InvalidOrder);
        }
        source_regions.push(descriptor.region);
    }
    Ok(descriptors)
}

pub(crate) fn validate_warm(
    source_units: &BTreeMap<SourceUnitId, ArtifactSourceUnit>,
    artifacts: &[CompileArtifact],
    stored: &[CustomBlockDescriptor],
    descriptor: &CustomBlockDescriptor,
) -> Result<(), CustomBlockDescriptorError> {
    let mut combined: Vec<CustomBlockDescriptor> = stored
        .iter()
        .filter(|existing| existing.id != descriptor.id)
        .cloned()
        .collect();
    combined.push(descriptor.clone());
    validate_attachment(source_units, artifacts, combined).map(|_| ())
}

fn check_against_set(
    source_units: &BTreeMap<SourceUnitId, ArtifactSourceUnit>,
    artifacts: &[CompileArtifact],
    descriptor: &CustomBlockDescriptor,
) -> Result<(), CustomBlockDescriptorError> {
    use CustomBlockDescriptorError as E;
    match descriptor.lifecycle {
        CustomBlockLifecycle::Complete => {}
        CustomBlockLifecycle::Partial => return Err(E::Partial),
        CustomBlockLifecycle::Cancelled => return Err(E::Cancelled),
        CustomBlockLifecycle::Stale => return Err(E::Stale),
    }
    let Some(source) = source_units.get(&descriptor.source_unit) else {
        return Err(E::UnknownSourceUnit);
    };
    if descriptor.source_id != *source.unit.source_id()
        || descriptor.source_content != *source.unit.content()
    {
        return Err(E::SourceMismatch);
    }
    if descriptor.revision != *source.unit.revision() {
        return Err(E::Stale);
    }
    if descriptor.region.start < source.source_span.start
        || descriptor.region.end > source.source_span.end
    {
        return Err(E::InvalidRegion);
    }
    if descriptor
        .provenance
        .inputs
        .iter()
        .any(|id| !source_units.contains_key(id))
    {
        return Err(E::UnknownSourceUnit);
    }
    let Some(target) = artifacts
        .iter()
        .find(|artifact| artifact.id() == &descriptor.attached_to)
    else {
        return Err(E::UnknownRelationTarget);
    };
    let Some(target_source) = source_units.get(target.source_unit()) else {
        return Err(E::UnknownSourceUnit);
    };
    if descriptor.source_id != *target_source.unit.source_id() {
        return Err(E::SourceMismatch);
    }
    Ok(())
}

fn spans_overlap(left: Span, right: Span) -> bool {
    left.start < right.end && right.start < left.end
}

/// One fixture block for [`custom_block_fixture_set`]: the source facts a
/// consumer characterizes, minus the set scaffolding the mint owns.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
pub struct CustomBlockFixture {
    pub role: String,
    pub lang: Option<String>,
    pub src: Option<String>,
    /// Authored inline bytes. `None` with `src` mints [`CustomBlockContent::SrcBacked`];
    /// `None` without `src` mints [`CustomBlockContent::Empty`].
    pub text: Option<String>,
}

#[cfg(any(test, feature = "test-support"))]
impl CustomBlockFixture {
    pub fn local(role: &str, text: &str) -> Self {
        Self {
            role: role.to_string(),
            lang: None,
            src: None,
            text: Some(text.to_string()),
        }
    }

    pub fn src_backed(role: &str, src: &str) -> Self {
        Self {
            role: role.to_string(),
            lang: None,
            src: Some(src.to_string()),
            text: None,
        }
    }

    pub fn empty(role: &str) -> Self {
        Self {
            role: role.to_string(),
            lang: None,
            src: None,
            text: None,
        }
    }
}

/// Test-only mint of a descriptor set over `source`: one `"sfc"` source
/// unit, one `"sfc"` analysis artifact, and one attached descriptor per
/// fixture in the given order, with regions anchored sequentially inside
/// the staged source span. Production mints go through the producing
/// bridge, which binds registered source lineage this fixture cannot know.
#[cfg(any(test, feature = "test-support"))]
pub fn custom_block_fixture_set(
    source: &str,
    blocks: Vec<CustomBlockFixture>,
) -> Result<super::publish::CompileArtifactSet, CustomBlockDescriptorError> {
    use super::fragment::{
        ArtifactContent, ArtifactProvenance, ArtifactUnavailableReason, CompileArtifact,
    };
    use super::publish::CompileArtifactSet;
    use super::source_unit::{carrier_revision_of, ArtifactSourceUnit, SourceUnit};
    use verter_identity::identity::{InputBasisId, ResultContractId};
    use verter_language::LanguageId;

    struct FixtureBasis;
    impl verter_identity::encoding::CanonicalEncode for FixtureBasis {
        const DOMAIN_TAG: &'static str = "verter.compiler.custom_block.fixture.basis.v1";
        fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
            e.field_str(1, "fixture");
        }
    }
    struct FixtureProducer;
    impl verter_identity::encoding::CanonicalEncode for FixtureProducer {
        const DOMAIN_TAG: &'static str = "verter.compiler.custom_block.fixture.producer.v1";
        fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
            e.field_str(1, "fixture-custom-blocks");
        }
    }

    let content = ContentId::from_content_bytes(source.as_bytes());
    let unit = SourceUnit::mint(
        SourceId::from_canonical(&FixtureBasis),
        carrier_revision_of(&content),
        "sfc",
        content.clone(),
    );
    let provenance = ArtifactProvenance {
        input_basis: InputBasisId::from_canonical(&FixtureBasis),
        producer: ResultContractId::from_canonical(&FixtureProducer),
        inputs: std::collections::BTreeSet::from([unit.id().clone()]),
    };
    let sfc = CompileArtifact::new(
        unit.id().clone(),
        crate::compile_request::ProductKind::Analysis,
        LanguageId::new("vue"),
        "sfc",
        provenance.clone(),
        ArtifactContent::Unavailable(ArtifactUnavailableReason::NotProduced),
    );
    let mut anchor = 0u32;
    let descriptors = blocks
        .into_iter()
        .enumerate()
        .map(|(order, fixture)| {
            let (block_content, region_len) = match (&fixture.text, fixture.src.as_deref()) {
                (Some(text), None) => (
                    CustomBlockContent::Local {
                        content: ContentId::from_content_bytes(text.as_bytes()),
                        text: text.clone(),
                    },
                    text.len() as u32,
                ),
                (None, Some(_)) => (CustomBlockContent::SrcBacked, 0),
                (None, None) => (CustomBlockContent::Empty, 0),
                (Some(_), Some(_)) => return Err(CustomBlockDescriptorError::Malformed),
            };
            let mut attributes: Vec<(String, String)> = Vec::new();
            if let Some(lang) = fixture.lang.as_deref() {
                attributes.push(("lang".to_string(), lang.to_string()));
            }
            if let Some(src) = fixture.src.as_deref() {
                attributes.push(("src".to_string(), src.to_string()));
            }
            let region = Span::new(anchor, anchor + region_len);
            // Zero-length regions anchor at strictly increasing positions so
            // sibling order validation holds for content-less blocks.
            anchor += region_len.max(1);
            let descriptor = CustomBlockDescriptor::try_new(CustomBlockDescriptorRequest {
                source_unit: unit.id().clone(),
                source_id: unit.source_id().clone(),
                revision: unit.revision().clone(),
                source_content: content.clone(),
                role: fixture.role,
                lang: fixture.lang,
                src: fixture.src,
                attributes,
                source_order: order as u32,
                region,
                content: block_content,
                provenance: provenance.clone(),
                attached_to: sfc.id().clone(),
                lifecycle: CustomBlockLifecycle::Complete,
            })?;
            Ok(descriptor)
        })
        .collect::<Result<Vec<_>, _>>()?;
    CompileArtifactSet::new(
        vec![ArtifactSourceUnit {
            unit,
            source_span: Span::new(0, source.len() as u32),
        }],
        vec![sfc],
    )
    .map_err(|_| CustomBlockDescriptorError::Malformed)?
    .attach_custom_blocks(descriptors)
}
