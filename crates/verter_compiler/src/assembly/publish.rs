//! Atomic artifact-set publication. `publish` either returns EXACTLY the
//! planned artifact set with every contract-required mapping product
//! attached, or a typed [`AssemblyRefusal`] and nothing — no variant
//! carries a partial/publishable sibling artifact, and no artifact is
//! returned that was not in [`ProductPlan`].

use super::custom_block::{CustomBlockDescriptor, CustomBlockDescriptorError};
use super::fragment::{DeclaredHelper, DeclaredImport, FragmentDialect, ValidatedFragment};
use super::plan::ProductPlan;
use crate::compile_request::ProductKind;

/// A schema invariant failed; no partial `CompileArtifactSet` is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSchemaError {
    DuplicateArtifact,
    DuplicateSourceUnit,
    ConflictingSourceRevision,
    InvalidSourceSpan,
    InvalidIdentity,
    UnknownSourceUnit,
    MissingPrimaryInput,
    MissingRelationTarget,
    SelfRelation,
    DuplicateMapFamily,
    UnqualifiedMap,
    WrongGeneratedSpace,
    StaleGeneratedContent,
    StaleMapInputBasis,
    MapSourceOutsideProvenance,
    UnavailableMappedContent,
    InvalidGeneratedRange,
    InvalidSourceRange,
    OverlappingMappings,
    /// A custom-block descriptor request failed validated construction or
    /// set attachment (malformed fact, aliased attribute, order/region
    /// conflict). Carries the precise cause rather than silently dropping
    /// every sibling descriptor.
    CustomBlockInvalid(CustomBlockDescriptorError),
    /// A staged handoff named a root artifact the set does not contain.
    UnknownRootArtifact,
    /// A staged handoff's root artifact carries no produced content.
    UnavailableRootArtifact,
}

impl std::fmt::Display for ArtifactSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid compile artifact schema: {self:?}")
    }
}
impl std::error::Error for ArtifactSchemaError {}

/// Immutable compiler-owned schema over already-produced facts. Creation
/// validates references and coordinate qualification, and sorts set-valued
/// data; it performs no parsing, lowering, code generation or publication.
/// Only validated sets serialize. This is not an assembly/publication receipt.
#[derive(Debug, Clone)]
pub struct CompileArtifactSet {
    source_units: std::collections::BTreeMap<
        super::source_unit::SourceUnitId,
        super::source_unit::ArtifactSourceUnit,
    >,
    artifacts: Vec<super::fragment::CompileArtifact>,
    custom_blocks: Vec<CustomBlockDescriptor>,
}

impl CompileArtifactSet {
    pub fn new(
        source_units: Vec<super::source_unit::ArtifactSourceUnit>,
        mut artifacts: Vec<super::fragment::CompileArtifact>,
    ) -> Result<Self, ArtifactSchemaError> {
        use std::collections::{BTreeMap, BTreeSet};
        use ArtifactSchemaError as E;
        let mut sources = BTreeMap::new();
        let mut revisions = BTreeMap::new();
        for source in source_units {
            if source.source_span.start > source.source_span.end {
                return Err(E::InvalidSourceSpan);
            }
            if source.unit.logical_role().is_empty() {
                return Err(E::InvalidIdentity);
            }
            if let Some(revision) = revisions.insert(
                source.unit.source_id().clone(),
                source.unit.revision().clone(),
            ) {
                if revision != *source.unit.revision() {
                    return Err(E::ConflictingSourceRevision);
                }
            }
            if sources.insert(source.unit.id().clone(), source).is_some() {
                return Err(E::DuplicateSourceUnit);
            }
        }
        artifacts.sort_by(|a, b| a.id().cmp(b.id()));
        let ids: BTreeSet<_> = artifacts.iter().map(|a| a.id().clone()).collect();
        if ids.len() != artifacts.len() {
            return Err(E::DuplicateArtifact);
        }
        for artifact in &mut artifacts {
            if artifact.name().is_empty() || artifact.language().as_str().is_empty() {
                return Err(E::InvalidIdentity);
            }
            if !sources.contains_key(artifact.source_unit()) {
                return Err(E::UnknownSourceUnit);
            }
            if !artifact.provenance.inputs.contains(artifact.source_unit()) {
                return Err(E::MissingPrimaryInput);
            }
            if artifact
                .provenance
                .inputs
                .iter()
                .any(|id| !sources.contains_key(id))
            {
                return Err(E::UnknownSourceUnit);
            }
            for relation in &artifact.relations {
                if &relation.target == artifact.id() {
                    return Err(E::SelfRelation);
                }
                if !ids.contains(&relation.target) {
                    return Err(E::MissingRelationTarget);
                }
            }
            artifact.maps.sort_by_key(|map| map.family);
            if artifact
                .maps
                .windows(2)
                .any(|pair| pair[0].family == pair[1].family)
            {
                return Err(E::DuplicateMapFamily);
            }
            let id = artifact.id().clone();
            // Hash once per mapped artifact, shared by its independent map
            // families. This binds geometry to bytes without invoking a compiler.
            let generated_content = match (&artifact.content, artifact.maps.is_empty()) {
                (super::fragment::ArtifactContent::Available(code), false) => Some(
                    super::source_unit::ContentId::from_content_bytes(code.as_bytes()),
                ),
                _ => None,
            };
            for map in &mut artifact.maps {
                map.validate(
                    &id,
                    &artifact.content,
                    &artifact.provenance,
                    generated_content.as_ref(),
                    &sources,
                )?;
            }
        }
        Ok(Self {
            source_units: sources,
            artifacts,
            custom_blocks: Vec::new(),
        })
    }

    pub fn custom_blocks(&self) -> &[CustomBlockDescriptor] {
        &self.custom_blocks
    }

    /// Attach validated descriptors with typed `attachedTo` relations.
    /// Subsequent calls merge with already-attached descriptors and re-run
    /// identity, order, overlap, and source checks across the combined set.
    /// Stale, cancelled, partial, or source-mismatched descriptors fail closed.
    pub fn attach_custom_blocks(
        mut self,
        descriptors: Vec<CustomBlockDescriptor>,
    ) -> Result<Self, CustomBlockDescriptorError> {
        let mut combined = std::mem::take(&mut self.custom_blocks);
        combined.extend(descriptors);
        self.custom_blocks = super::custom_block::validate_attachment(
            &self.source_units,
            &self.artifacts,
            combined,
        )?;
        Ok(self)
    }

    /// Warm gate: the same fail-closed checks as attachment, including
    /// set-level identity, order, overlap, and region-order rules against
    /// already-attached descriptors. Independent of whether `descriptor` is
    /// already stored on this set — an identical stored id is not treated as
    /// a duplicate of itself.
    pub fn warm_custom_block(
        &self,
        descriptor: &CustomBlockDescriptor,
    ) -> Result<(), CustomBlockDescriptorError> {
        super::custom_block::validate_warm(
            &self.source_units,
            &self.artifacts,
            &self.custom_blocks,
            descriptor,
        )
    }

    pub fn source_units(
        &self,
    ) -> impl ExactSizeIterator<Item = &super::source_unit::ArtifactSourceUnit> {
        self.source_units.values()
    }

    pub fn artifacts(&self) -> &[super::fragment::CompileArtifact] {
        &self.artifacts
    }

    pub fn artifact(
        &self,
        id: &super::fragment::ArtifactId,
    ) -> Option<&super::fragment::CompileArtifact> {
        self.artifacts
            .binary_search_by(|a| a.id().cmp(id))
            .ok()
            .map(|index| &self.artifacts[index])
    }

    /// Deterministic terminal JSON. Identities retain full canonical bytes as
    /// hex, not digest-only equality or Debug text. Coordinates remain bytes;
    /// a transport requiring UTF-16 must convert using its source documents.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl serde::Serialize for CompileArtifactSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use super::fragment::ArtifactContent;
        use serde_json::{json, Value};
        let sources: Vec<_> = self
            .source_units
            .values()
            .map(|s| {
                json!({
                    "id": hex::encode(s.unit.id().canonical_bytes()),
                    "source": hex::encode(s.unit.source_id().canonical_bytes()),
                    "revision": hex::encode(s.unit.revision().canonical_bytes()),
                    "content": hex::encode(s.unit.content().canonical_bytes()),
                    "role": s.unit.logical_role(), "sourceBytes": s.source_span,
                })
            })
            .collect();
        let artifacts: Vec<_> = self.artifacts.iter().map(|a| {
            let content = match &a.content {
                ArtifactContent::Available(text) => {
                    json!({"availability": "available", "text": text.as_ref()})
                }
                ArtifactContent::Unavailable(reason) => json!({"availability": "unavailable", "reason": reason}),
            };
            let relations: Vec<_> = a.relations.iter().map(|r| json!({
                "kind": r.kind, "target": hex::encode(r.target.canonical_bytes()),
            })).collect();
            let maps: Vec<_> = a.maps.iter().map(|m| {
                let segments: Vec<Value> = m.segments.iter().map(|s| json!({
                    "generatedBytes": {"start": s.generated.start, "end": s.generated.end},
                    "sourceUnit": hex::encode(s.source_unit.canonical_bytes()), "sourceBytes": s.source_span,
                })).collect();
                json!({
                    "family": m.family, "generated": hex::encode(m.generated.canonical_bytes()),
                    "generatedContent": hex::encode(m.generated_content.canonical_bytes()),
                    "inputBasis": hex::encode(m.input_basis.canonical_bytes()),
                    "sources": m.sources.iter().map(|s| hex::encode(s.canonical_bytes())).collect::<Vec<_>>(),
                    "segments": segments,
                })
            }).collect();
            json!({
                "id": hex::encode(a.id().canonical_bytes()), "sourceUnit": hex::encode(a.source_unit().canonical_bytes()),
                "product": a.product().wire_tag(), "language": a.language().as_str(), "name": a.name(),
                "provenance": {
                    "inputBasis": hex::encode(a.provenance.input_basis.canonical_bytes()),
                    "producer": hex::encode(a.provenance.producer.canonical_bytes()),
                    "inputs": a.provenance.inputs.iter().map(|s| hex::encode(s.canonical_bytes())).collect::<Vec<_>>(),
                },
                "content": content, "relations": relations, "maps": maps,
            })
        }).collect();
        let custom_blocks: Vec<_> = self.custom_blocks.iter().map(|block| {
            use super::custom_block::CustomBlockContent;
            let content = match block.content() {
                CustomBlockContent::Local { content, text } => json!({
                    "availability": "local",
                    "text": text,
                    "content": hex::encode(content.canonical_bytes()),
                }),
                CustomBlockContent::SrcBacked => json!({"availability": "srcBacked"}),
                CustomBlockContent::Empty => json!({"availability": "empty"}),
                CustomBlockContent::Unavailable(reason) => {
                    json!({"availability": "unavailable", "reason": reason})
                }
            };
            json!({
                "id": hex::encode(block.id().canonical_bytes()),
                "sourceUnit": hex::encode(block.source_unit().canonical_bytes()),
                "source": hex::encode(block.source_id().canonical_bytes()),
                "revision": hex::encode(block.revision().canonical_bytes()),
                "sourceContent": hex::encode(block.source_content().canonical_bytes()),
                "role": block.role(),
                "lang": block.lang(),
                "src": block.src(),
                "attributes": block.attributes().iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>(),
                "sourceOrder": block.source_order(),
                "region": block.region(),
                "content": content,
                "provenance": {
                    "inputBasis": hex::encode(block.provenance().input_basis.canonical_bytes()),
                    "producer": hex::encode(block.provenance().producer.canonical_bytes()),
                    "inputs": block.provenance().inputs.iter().map(|s| hex::encode(s.canonical_bytes())).collect::<Vec<_>>(),
                },
                "relation": {
                    "kind": "attachedTo",
                    "target": hex::encode(block.attached_to().canonical_bytes()),
                },
                "lifecycle": match block.lifecycle() {
                    super::custom_block::CustomBlockLifecycle::Complete => "complete",
                    super::custom_block::CustomBlockLifecycle::Partial => "partial",
                    super::custom_block::CustomBlockLifecycle::Cancelled => "cancelled",
                    super::custom_block::CustomBlockLifecycle::Stale => "stale",
                },
            })
        }).collect();
        let mut terminal = json!({"schemaVersion": 1, "coordinates": "utf8-bytes", "sourceUnits": sources, "artifacts": artifacts, "customBlocks": custom_blocks});
        // Keep wire order stable even when another crate enables serde_json's
        // preserve_order feature through Cargo feature unification.
        terminal.sort_all_objects();
        terminal.serialize(serializer)
    }
}

/// One request's COMPLETE staged compile handoff — the single value a
/// framework host-integration backend passes to session lifecycle and
/// publication code.
///
/// It carries the validated [`CompileArtifactSet`], the typed identity of
/// the runtime-module artifact inside it, the dialect that artifact's
/// bytes are written in, and the runtime source map produced with them.
/// Every fact the host needs in order to publish the runtime module is
/// read off THIS value: a consumer neither locates the module's artifact
/// by scanning names nor re-derives its language from carrier metadata.
///
/// The module bytes live ON the root artifact rather than beside it, so
/// "a published body without its typed artifact set" is not representable.
/// [`Self::stage`] is the only mint site and validates the root, so
/// "a staged handoff whose root is absent or unproduced" is not
/// representable either.
#[derive(Debug, Clone)]
pub struct StagedCompileArtifacts {
    artifacts: CompileArtifactSet,
    root: super::fragment::ArtifactId,
    dialect: FragmentDialect,
    source_map: Option<String>,
}

impl StagedCompileArtifacts {
    /// Stage `artifacts` for host handoff with `root` as its runtime module.
    ///
    /// An empty `source_map` stages as absent: downstream carries the map
    /// demand by presence alone, so an empty JSON payload would read as a
    /// produced-but-empty map rather than as "no map was requested".
    ///
    /// # Errors
    ///
    /// [`ArtifactSchemaError::UnknownRootArtifact`] when `root` names no
    /// artifact in `artifacts`; [`ArtifactSchemaError::UnavailableRootArtifact`]
    /// when the named artifact carries no produced content.
    pub fn stage(
        artifacts: CompileArtifactSet,
        root: super::fragment::ArtifactId,
        dialect: FragmentDialect,
        source_map: Option<String>,
    ) -> Result<Self, ArtifactSchemaError> {
        use super::fragment::ArtifactContent;
        match artifacts.artifact(&root).map(|artifact| &artifact.content) {
            None => return Err(ArtifactSchemaError::UnknownRootArtifact),
            Some(ArtifactContent::Unavailable(_)) => {
                return Err(ArtifactSchemaError::UnavailableRootArtifact)
            }
            Some(ArtifactContent::Available(_)) => {}
        }
        Ok(Self {
            artifacts,
            root,
            dialect,
            source_map: source_map.filter(|json| !json.is_empty()),
        })
    }

    /// The complete staged artifact set, with every source unit, typed
    /// relation and qualified map the compiler produced for this request.
    pub fn set(&self) -> &CompileArtifactSet {
        &self.artifacts
    }

    /// The runtime-module artifact this handoff publishes.
    pub fn root(&self) -> &super::fragment::CompileArtifact {
        self.artifacts
            .artifact(&self.root)
            .expect("`stage` rejects a root absent from the set, which is immutable afterwards")
    }

    /// The runtime module's bytes — the SAME allocation the root artifact
    /// stores, so publishing shares it rather than copying the source.
    pub fn code(&self) -> &std::sync::Arc<str> {
        match &self.root().content {
            super::fragment::ArtifactContent::Available(code) => code,
            super::fragment::ArtifactContent::Unavailable(_) => unreachable!(
                "`stage` rejects an unavailable root, and the set is immutable afterwards"
            ),
        }
    }

    /// The exact dialect [`Self::code`] is written in, declared by the
    /// producing assembly rather than inferred downstream.
    pub fn dialect(&self) -> FragmentDialect {
        self.dialect
    }

    /// The language id (`"js"`/`"jsx"`/`"ts"`/`"tsx"`) of [`Self::code`].
    pub fn lang(&self) -> &'static str {
        self.dialect.lang_id()
    }

    /// The product this runtime module was compiled as.
    pub fn product(&self) -> ProductKind {
        self.root().product()
    }

    /// The runtime source map for [`Self::code`]; `None` when no map was
    /// demanded or produced.
    pub fn source_map(&self) -> Option<&str> {
        self.source_map.as_deref()
    }
}

/// One artifact's finished composition, reported by the caller (the
/// framework-owned composer) together with the exact facts `publish` needs
/// to verify atomicity — never re-derived by scanning `code`.
///
/// `pub(crate)`: the ONLY way to mint an [`ArtifactSet`] from outside this
/// crate is [`crate::standalone::StandaloneCompiler::compile`] — no external
/// crate constructs a contribution and calls [`publish`] independently, so
/// the raw-source direct route is structurally closed.
pub(crate) struct ArtifactContribution<'a> {
    pub kind: ProductKind,
    /// Every fragment that contributed to this artifact — the declared
    /// helper/import union this artifact's own emitted imports are
    /// checked against, and the SAME collection `assemble_sequence`/
    /// `splice_into_hole` composed `code` from (not a second, separately
    /// maintained list).
    pub fragments: Vec<&'a ValidatedFragment>,
    pub code: String,
    /// The import statements the composer actually wrote into `code` —
    /// a fact the composer already knows because it is the one writing
    /// them, reported here rather than recovered by parsing `code` back.
    pub emitted_imports: Vec<DeclaredImport>,
    /// The exact dialect [`Self::code`] is written in — the SAME dialect
    /// every contributing fragment declared and validated under (a
    /// mismatch there is a producer bug, not this check's concern); used
    /// for the final-parse check below instead of a fixed permissive
    /// default.
    pub dialect: FragmentDialect,
    pub source_projection_map: Option<String>,
    pub runtime_source_map: Option<String>,
}

/// A published artifact. Fields are private — the only mint site is
/// [`publish`]'s own atomicity checks, so a caller cannot hand-build a
/// value that LOOKS validated without actually going through them (the
/// same sealing discipline [`ValidatedFragment`](super::fragment::ValidatedFragment)
/// already holds for a fragment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledArtifact {
    kind: ProductKind,
    code: String,
    dialect: FragmentDialect,
    source_projection_map: Option<String>,
    runtime_source_map: Option<String>,
}

impl AssembledArtifact {
    pub fn kind(&self) -> ProductKind {
        self.kind
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// The exact ECMAScript/TypeScript dialect [`Self::code`] is written in
    /// — the same [`FragmentDialect`] its publishing
    /// [`ArtifactContribution::dialect`] declared. A caller with no other
    /// route to this fact (a raw-source direct-core consumer has no
    /// [`crate::parser::types::ParsedSfc`] of its own to re-derive it from)
    /// reads it here rather than re-parsing the carrier a second time.
    pub fn dialect(&self) -> FragmentDialect {
        self.dialect
    }

    pub fn source_projection_map(&self) -> Option<&str> {
        self.source_projection_map.as_deref()
    }

    pub fn runtime_source_map(&self) -> Option<&str> {
        self.runtime_source_map.as_deref()
    }
}

/// No `Default` — an `ArtifactSet` exists only as [`publish`]'s return
/// value. A publishable-looking empty set that never went through
/// `publish`'s atomicity checks would be indistinguishable from a
/// genuinely empty (zero-product) publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    artifacts: Vec<AssembledArtifact>,
}

impl ArtifactSet {
    pub fn artifacts(&self) -> &[AssembledArtifact] {
        &self.artifacts
    }

    pub fn artifact(&self, kind: ProductKind) -> Option<&AssembledArtifact> {
        self.artifacts.iter().find(|a| a.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyRefusal {
    /// The plan named an artifact no contribution was supplied for.
    MissingPlannedArtifact { kind: ProductKind },
    /// A contribution was supplied for an artifact the plan never
    /// requested.
    UnplannedArtifactProduced { kind: ProductKind },
    /// An `IdeCompanion` (or any artifact whose plan entry requires one)
    /// was composed without its non-optional `SourceProjectionMap`.
    MissingRequiredSourceProjectionMap { kind: ProductKind },
    /// A `SourceProjectionMap` was attached to an artifact the plan did
    /// not mark as requiring one — never a default-constructed extra.
    UnrequestedSourceProjectionMap { kind: ProductKind },
    /// A runtime product requested `runtime_source_map` but its
    /// contribution carries none.
    MissingRequiredRuntimeSourceMap { kind: ProductKind },
    /// A `RuntimeSourceMapData` was attached without having been
    /// requested.
    UnrequestedRuntimeSourceMap { kind: ProductKind },
    /// An artifact emits an import no contributing fragment declared.
    UndeclaredHelper {
        kind: ProductKind,
        specifier: String,
        name: String,
    },
    /// More than one contribution was supplied for the same planned
    /// [`ProductKind`] — "exactly the planned set" means output
    /// cardinality matches the plan exactly, not "at least" it.
    DuplicateArtifactContribution { kind: ProductKind },
    /// A code-bearing artifact's final composed bytes do not parse as a
    /// complete ECMAScript/TypeScript module.
    FinalParseFailed { kind: ProductKind, reason: String },
}

/// Product kinds whose [`ArtifactContribution::code`] is required to be a
/// real ECMAScript/TypeScript module — the ones [`publish`]'s final-parse
/// check applies to. `PublicApi`/`Analysis` may carry a non-code payload
/// (facts/serialized data), so they are not parsed as JS here.
fn is_code_bearing(kind: ProductKind) -> bool {
    matches!(
        kind,
        ProductKind::RuntimeClient
            | ProductKind::RuntimeServer
            | ProductKind::IdeCompanion
            | ProductKind::Declarations
    )
}

fn declared_names<'a>(fragments: &[&'a ValidatedFragment], specifier: &str) -> Vec<&'a str> {
    fragments
        .iter()
        .flat_map(|fragment| {
            let f = fragment.fragment();
            let from_imports = f
                .imports
                .iter()
                .filter(|i: &&DeclaredImport| i.specifier == specifier)
                .flat_map(|i: &DeclaredImport| i.bound_names());
            let from_helpers = f.helpers.iter().map(|h: &DeclaredHelper| h.name.as_str());
            from_imports.chain(from_helpers)
        })
        .collect()
}

/// Validate `contributions` against `plan` and publish atomically. Every
/// check below runs to completion against `contributions` before any
/// [`ArtifactSet`] is constructed — a failure anywhere returns exactly one
/// [`AssemblyRefusal`] and builds no artifact at all.
///
/// `pub(crate)`: [`crate::assembly::vue_module::compose_main_module`] and
/// [`crate::standalone::StandaloneCompiler::compile`] are this crate's only
/// callers — no legacy alternate core outside `verter_compiler` may publish
/// an [`ArtifactSet`] directly.
pub(crate) fn publish(
    plan: &ProductPlan,
    contributions: Vec<ArtifactContribution<'_>>,
) -> Result<ArtifactSet, AssemblyRefusal> {
    // Exact cardinality: two contributions for the same kind would let one
    // shadow the other in the published set (`ArtifactSet::artifact` finds
    // the FIRST match) while the second's own atomicity checks silently
    // ran and passed for nothing — "exactly the planned set" means output
    // cardinality matches the plan exactly, never "at least."
    let mut seen_kinds = std::collections::HashSet::new();
    for contribution in &contributions {
        if !seen_kinds.insert(contribution.kind) {
            return Err(AssemblyRefusal::DuplicateArtifactContribution {
                kind: contribution.kind,
            });
        }
    }

    for planned in plan.artifacts() {
        if !contributions.iter().any(|c| c.kind == planned.kind) {
            return Err(AssemblyRefusal::MissingPlannedArtifact { kind: planned.kind });
        }
    }
    for contribution in &contributions {
        let Some(planned) = plan.artifact(contribution.kind) else {
            return Err(AssemblyRefusal::UnplannedArtifactProduced {
                kind: contribution.kind,
            });
        };

        match (
            planned.requires_source_projection_map,
            &contribution.source_projection_map,
        ) {
            (true, None) => {
                return Err(AssemblyRefusal::MissingRequiredSourceProjectionMap {
                    kind: contribution.kind,
                })
            }
            (false, Some(_)) => {
                return Err(AssemblyRefusal::UnrequestedSourceProjectionMap {
                    kind: contribution.kind,
                })
            }
            _ => {}
        }

        match (
            planned.requires_runtime_source_map,
            &contribution.runtime_source_map,
        ) {
            (true, None) => {
                return Err(AssemblyRefusal::MissingRequiredRuntimeSourceMap {
                    kind: contribution.kind,
                })
            }
            (false, Some(_)) => {
                return Err(AssemblyRefusal::UnrequestedRuntimeSourceMap {
                    kind: contribution.kind,
                })
            }
            _ => {}
        }

        for emitted in &contribution.emitted_imports {
            let declared: Vec<&str> = declared_names(&contribution.fragments, &emitted.specifier);
            for name in emitted.bound_names() {
                if !declared.contains(&name) {
                    return Err(AssemblyRefusal::UndeclaredHelper {
                        kind: contribution.kind,
                        specifier: emitted.specifier.clone(),
                        name: name.to_string(),
                    });
                }
            }
        }

        // Final assembly must parse as its declared ECMAScript/TypeScript
        // module — checked here, once, on the fully-composed bytes, not
        // inferred from any contributing fragment individually having
        // parsed. `contribution.dialect`, not a fixed permissive default —
        // a JS artifact must reject TypeScript-only syntax rather than
        // silently accept it under TSX.
        if is_code_bearing(contribution.kind) {
            if let Some(reason) =
                super::fragment::final_module_parse_errors(&contribution.code, contribution.dialect)
            {
                return Err(AssemblyRefusal::FinalParseFailed {
                    kind: contribution.kind,
                    reason,
                });
            }
        }
    }

    let artifacts = contributions
        .into_iter()
        .map(|c| AssembledArtifact {
            kind: c.kind,
            code: c.code,
            dialect: c.dialect,
            source_projection_map: c.source_projection_map,
            runtime_source_map: c.runtime_source_map,
        })
        .collect();
    Ok(ArtifactSet { artifacts })
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
