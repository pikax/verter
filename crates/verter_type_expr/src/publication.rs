//! Resolved type authority and pure publication selection.

use std::sync::Arc;

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::facts::{SchemaAbsence, SemanticSourceFailure, SemanticTypeSource, SourcePosition};
use crate::locators::{AuthoredBodyLocator, MacroPayloadLocator};

/// Semantic exactness of the resolved authority.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionExactness {
    ExactConcrete,
    ExactSymbolic,
    Incomplete,
}

/// Typed failure of a required resolution position.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum TypedResolutionFailure {
    SourceConstruction(SemanticSourceFailure),
}

/// Origin of the resolved authority. This is producer data, not inferred from
/// terminal text.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionProvenance {
    SemanticEvaluator,
    SessionProjector,
    FrameworkSurface,
    FallthroughInheritance,
    Schema,
}

/// Closed diagnostic classification carried by resolved authority.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionDiagnosticKind {
    BudgetExceeded,
    ProjectionWorkLimit,
    ConnectedQueryDepthLimit,
    MappedDepthExceeded,
    UnresolvedReference,
    IndeterminateConditional,
    InfiniteKeySpace,
    UnsupportedOperator,
    ConditionalContextTruncated,
    IdempotentArm,
    CyclicReference,
    CyclicInstantiation,
    InstantiationError,
    EmptyUnionArm,
}

/// One typed resolution diagnostic.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionDiagnostic {
    pub kind: ResolutionDiagnosticKind,
    pub context: Arc<str>,
    pub property_name: Option<Arc<str>>,
}

/// The immutable result authority before representation selection.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ResolvedTypeOutcome {
    Present {
        source: Arc<SemanticTypeSource>,
        exactness: ResolutionExactness,
    },
    Absent {
        absence: SchemaAbsence,
    },
    Failed {
        failure: TypedResolutionFailure,
    },
}

/// Resolved semantic authority. Authored spelling selection never mutates it.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeAuthority {
    outcome: ResolvedTypeOutcome,
    provenance: ResolutionProvenance,
    diagnostics: Arc<[ResolutionDiagnostic]>,
}

impl ResolvedTypeAuthority {
    #[must_use]
    pub fn present(
        source: SemanticTypeSource,
        exactness: ResolutionExactness,
        provenance: ResolutionProvenance,
        diagnostics: Arc<[ResolutionDiagnostic]>,
    ) -> Self {
        Self {
            outcome: ResolvedTypeOutcome::Present {
                source: Arc::new(source),
                exactness,
            },
            provenance,
            diagnostics,
        }
    }

    #[must_use]
    pub fn absent(
        absence: SchemaAbsence,
        provenance: ResolutionProvenance,
        diagnostics: Arc<[ResolutionDiagnostic]>,
    ) -> Self {
        Self {
            outcome: ResolvedTypeOutcome::Absent { absence },
            provenance,
            diagnostics,
        }
    }

    #[must_use]
    pub fn failed(
        failure: TypedResolutionFailure,
        provenance: ResolutionProvenance,
        diagnostics: Arc<[ResolutionDiagnostic]>,
    ) -> Self {
        Self {
            outcome: ResolvedTypeOutcome::Failed { failure },
            provenance,
            diagnostics,
        }
    }

    #[must_use]
    pub fn from_source_position(
        position: &SourcePosition,
        exactness: ResolutionExactness,
        provenance: ResolutionProvenance,
        diagnostics: Arc<[ResolutionDiagnostic]>,
    ) -> Self {
        match position {
            SourcePosition::Present(source) => {
                Self::present(source.clone(), exactness, provenance, diagnostics)
            }
            SourcePosition::Absent(absence) => Self::absent(*absence, provenance, diagnostics),
            SourcePosition::Failed(failure) => Self::failed(
                TypedResolutionFailure::SourceConstruction(*failure),
                provenance,
                diagnostics,
            ),
        }
    }

    #[must_use]
    pub fn outcome(&self) -> &ResolvedTypeOutcome {
        &self.outcome
    }

    #[must_use]
    pub fn provenance(&self) -> ResolutionProvenance {
        self.provenance
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[ResolutionDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn exactness(&self) -> Option<ResolutionExactness> {
        match self.outcome {
            ResolvedTypeOutcome::Present { exactness, .. } => Some(exactness),
            ResolvedTypeOutcome::Absent { .. } | ResolvedTypeOutcome::Failed { .. } => None,
        }
    }

    #[must_use]
    pub fn source(&self) -> Option<&SemanticTypeSource> {
        match &self.outcome {
            ResolvedTypeOutcome::Present { source, .. } => Some(source.as_ref()),
            ResolvedTypeOutcome::Absent { .. } | ResolvedTypeOutcome::Failed { .. } => None,
        }
    }

    #[must_use]
    pub fn source_position(&self) -> SourcePosition {
        match &self.outcome {
            ResolvedTypeOutcome::Present { source, .. } => {
                SourcePosition::Present(source.as_ref().clone())
            }
            ResolvedTypeOutcome::Absent { absence } => SourcePosition::Absent(*absence),
            ResolvedTypeOutcome::Failed { failure } => match failure {
                TypedResolutionFailure::SourceConstruction(failure) => {
                    SourcePosition::Failed(*failure)
                }
            },
        }
    }
}

/// Locator-backed authored source capability. It cannot be constructed from
/// text or an arbitrary semantic source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(transparent)]
pub struct AuthoredTypeSource(AuthoredBodyLocator);

/// Capability held only by parser/analyzer producer boundaries that obtained
/// an authored locator and its text from the same producer row.
#[derive(Debug)]
pub struct AuthoredSourceMint {
    _private: (),
}

impl AuthoredSourceMint {
    /// Enter an audited authored-source producer boundary.
    ///
    /// # Safety
    ///
    /// The caller must own the parser/analyzer row that produced the locator
    /// and exact text. Consumers, merge code, and output code must not mint it.
    #[doc(hidden)]
    pub const unsafe fn new_unchecked() -> Self {
        Self { _private: () }
    }
}

impl AuthoredTypeSource {
    fn from_macro_payload(locator: &MacroPayloadLocator) -> Self {
        Self(AuthoredBodyLocator::MacroPayload(locator.clone()))
    }

    fn from_authored_body(locator: &AuthoredBodyLocator) -> Self {
        Self(locator.clone())
    }

    #[must_use]
    pub fn locator(&self) -> &AuthoredBodyLocator {
        &self.0
    }

    #[must_use]
    pub fn to_semantic_source(&self) -> SemanticTypeSource {
        SemanticTypeSource::Authored(self.0.clone())
    }
}

/// Structural producer provenance of authored evidence.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum AuthoredProvenance {
    MacroPayload,
    DeclarationBody,
    AugmentationBody,
    JsdocTypedefBody,
}

/// Inseparable authored locator, exact spelling, and provenance row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase")]
pub struct AuthoredTypeEvidence {
    source: AuthoredTypeSource,
    text: Arc<str>,
    provenance: AuthoredProvenance,
}

impl AuthoredTypeEvidence {
    #[must_use]
    pub fn from_macro_payload(
        _mint: &AuthoredSourceMint,
        locator: &MacroPayloadLocator,
        text: Arc<str>,
    ) -> Self {
        Self {
            source: AuthoredTypeSource::from_macro_payload(locator),
            text,
            provenance: AuthoredProvenance::MacroPayload,
        }
    }

    #[must_use]
    pub fn from_authored_body(
        _mint: &AuthoredSourceMint,
        locator: &AuthoredBodyLocator,
        text: Arc<str>,
    ) -> Self {
        let provenance = match locator {
            AuthoredBodyLocator::DeclBody(_) => AuthoredProvenance::DeclarationBody,
            AuthoredBodyLocator::AugmentationBody(_) => AuthoredProvenance::AugmentationBody,
            AuthoredBodyLocator::JsdocTypedefBody(_) => AuthoredProvenance::JsdocTypedefBody,
            AuthoredBodyLocator::MacroPayload(_) => AuthoredProvenance::MacroPayload,
        };
        Self {
            source: AuthoredTypeSource::from_authored_body(locator),
            text,
            provenance,
        }
    }

    #[must_use]
    pub fn source(&self) -> &AuthoredTypeSource {
        &self.source
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn provenance(&self) -> AuthoredProvenance {
        self.provenance
    }

    #[must_use]
    pub fn absolutized_against(&self, canonical_id: &str) -> Self {
        // SAFETY: this is the same already-minted producer row; only its
        // producer-local anchor is made owner-absolute.
        let mint = unsafe { AuthoredSourceMint::new_unchecked() };
        Self::from_authored_body(
            &mint,
            &self.source.locator().absolutized_against(canonical_id),
            Arc::clone(&self.text),
        )
    }
}

/// Structural reason the session policy permits authored selection.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum PublicationPolicyReason {
    ImportedMacroCompound,
    ImportedIndexedAccess,
}

/// Structural equivalence class proven by the session policy.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum SymbolicEquivalenceKind {
    ImportedMacroCompound,
    ImportedIndexedAccess,
}

/// Typed proof binding a resolved source to one authored representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase")]
pub struct SymbolicEquivalenceProof {
    kind: SymbolicEquivalenceKind,
    resolved_source: SemanticTypeSource,
    authored_source: AuthoredTypeSource,
}

/// Capability minted only on the success branch of the shared node-domain
/// lossless structural-projection validator.
#[derive(Debug)]
pub struct SymbolicEquivalenceMint {
    _private: (),
}

impl SymbolicEquivalenceMint {
    /// Enter the audited lossless structural-projection success branch.
    ///
    /// # Safety
    ///
    /// The caller must have raised both bound sources through the shared
    /// dispatch and observed `raised_shape_eq_nodes(...) == Some(true)`.
    #[doc(hidden)]
    pub const unsafe fn new_unchecked() -> Self {
        Self { _private: () }
    }
}

impl SymbolicEquivalenceProof {
    #[must_use]
    pub fn from_lossless_projection(
        _mint: &SymbolicEquivalenceMint,
        kind: SymbolicEquivalenceKind,
        resolved_source: SemanticTypeSource,
        authored_source: AuthoredTypeSource,
    ) -> Self {
        Self {
            kind,
            resolved_source,
            authored_source,
        }
    }

    #[must_use]
    pub fn kind(&self) -> SymbolicEquivalenceKind {
        self.kind
    }

    fn matches(
        &self,
        resolved_source: &SemanticTypeSource,
        authored_source: &AuthoredTypeSource,
    ) -> bool {
        self.resolved_source == *resolved_source && self.authored_source == *authored_source
    }

    fn absolutized_against(&self, canonical_id: &str) -> Self {
        Self {
            kind: self.kind,
            resolved_source: self.resolved_source.absolutized_against(canonical_id),
            authored_source: AuthoredTypeSource::from_authored_body(
                &self
                    .authored_source
                    .locator()
                    .absolutized_against(canonical_id),
            ),
        }
    }
}

/// Pure selector policy. `ExactOnly` is represented by no incomplete permit
/// and no symbolic proof.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase")]
pub struct PublicationPolicy {
    incomplete_reason: Option<PublicationPolicyReason>,
    symbolic_equivalence: Option<SymbolicEquivalenceProof>,
}

impl PublicationPolicy {
    #[must_use]
    pub const fn exact_only() -> Self {
        Self {
            incomplete_reason: None,
            symbolic_equivalence: None,
        }
    }

    #[must_use]
    pub const fn allow_authored_for_incomplete(reason: PublicationPolicyReason) -> Self {
        Self {
            incomplete_reason: Some(reason),
            symbolic_equivalence: None,
        }
    }

    #[must_use]
    pub fn with_symbolic_equivalence(mut self, proof: SymbolicEquivalenceProof) -> Self {
        self.symbolic_equivalence = Some(proof);
        self
    }

    #[must_use]
    pub fn incomplete_reason(&self) -> Option<PublicationPolicyReason> {
        self.incomplete_reason
    }
}

impl Default for PublicationPolicy {
    fn default() -> Self {
        Self::exact_only()
    }
}

/// Whether the selected representation is backed by resolved authority or an
/// explicitly-degraded authored fallback.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum SemanticAuthority {
    Resolved,
    AuthoredFallback,
}

/// Why one source was selected for publication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PublicationReason {
    ResolvedExactConcrete,
    ResolvedExactSymbolic,
    ResolvedIncomplete,
    AuthoredForIncomplete { policy: PublicationPolicy },
    AuthoredSymbolicRepresentation { proof: SymbolicEquivalenceProof },
}

/// Provenance of the published representation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PublicationProvenance {
    Resolved { provenance: ResolutionProvenance },
    Authored { provenance: AuthoredProvenance },
}

/// Pure publication outcome. Failure and absence remain structured and never
/// acquire a selected source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PublicationResult {
    Failed {
        failure: TypedResolutionFailure,
        provenance: ResolutionProvenance,
    },
    Absent {
        absence: SchemaAbsence,
        provenance: ResolutionProvenance,
    },
    Published {
        selected_source: Arc<SemanticTypeSource>,
        semantic_authority: SemanticAuthority,
        exactness: ResolutionExactness,
        reason: Box<PublicationReason>,
        provenance: PublicationProvenance,
    },
}

impl PublicationResult {
    #[must_use]
    pub fn selected_source(&self) -> Option<&SemanticTypeSource> {
        match self {
            Self::Published {
                selected_source, ..
            } => Some(selected_source.as_ref()),
            Self::Absent { .. } | Self::Failed { .. } => None,
        }
    }

    #[must_use]
    pub fn source_position(&self) -> SourcePosition {
        match self {
            Self::Published {
                selected_source, ..
            } => SourcePosition::Present(selected_source.as_ref().clone()),
            Self::Absent { absence, .. } => SourcePosition::Absent(*absence),
            Self::Failed { failure, .. } => match failure {
                TypedResolutionFailure::SourceConstruction(failure) => {
                    SourcePosition::Failed(*failure)
                }
            },
        }
    }
}

/// Select a representation without mutating resolved authority.
#[must_use]
pub fn select_type_publication(
    authority: &ResolvedTypeAuthority,
    evidence: Option<&AuthoredTypeEvidence>,
    policy: &PublicationPolicy,
) -> PublicationResult {
    let provenance = authority.provenance;
    match &authority.outcome {
        ResolvedTypeOutcome::Failed { failure } => PublicationResult::Failed {
            failure: *failure,
            provenance,
        },
        ResolvedTypeOutcome::Absent { absence } => PublicationResult::Absent {
            absence: *absence,
            provenance,
        },
        ResolvedTypeOutcome::Present {
            source,
            exactness: ResolutionExactness::ExactConcrete,
        } => resolved_publication(
            source,
            ResolutionExactness::ExactConcrete,
            PublicationReason::ResolvedExactConcrete,
            provenance,
        ),
        ResolvedTypeOutcome::Present {
            source,
            exactness: ResolutionExactness::ExactSymbolic,
        } => {
            if let (Some(evidence), Some(proof)) = (evidence, policy.symbolic_equivalence.as_ref())
            {
                if proof.matches(source, evidence.source()) {
                    return PublicationResult::Published {
                        selected_source: Arc::new(evidence.source().to_semantic_source()),
                        semantic_authority: SemanticAuthority::Resolved,
                        exactness: ResolutionExactness::ExactSymbolic,
                        reason: Box::new(PublicationReason::AuthoredSymbolicRepresentation {
                            proof: proof.clone(),
                        }),
                        provenance: PublicationProvenance::Authored {
                            provenance: evidence.provenance(),
                        },
                    };
                }
            }
            resolved_publication(
                source,
                ResolutionExactness::ExactSymbolic,
                PublicationReason::ResolvedExactSymbolic,
                provenance,
            )
        }
        ResolvedTypeOutcome::Present {
            source,
            exactness: ResolutionExactness::Incomplete,
        } => {
            if let (Some(evidence), Some(_)) = (evidence, policy.incomplete_reason) {
                return PublicationResult::Published {
                    selected_source: Arc::new(evidence.source().to_semantic_source()),
                    semantic_authority: SemanticAuthority::AuthoredFallback,
                    exactness: ResolutionExactness::Incomplete,
                    reason: Box::new(PublicationReason::AuthoredForIncomplete {
                        policy: policy.clone(),
                    }),
                    provenance: PublicationProvenance::Authored {
                        provenance: evidence.provenance(),
                    },
                };
            }
            resolved_publication(
                source,
                ResolutionExactness::Incomplete,
                PublicationReason::ResolvedIncomplete,
                provenance,
            )
        }
    }
}

fn resolved_publication(
    source: &Arc<SemanticTypeSource>,
    exactness: ResolutionExactness,
    reason: PublicationReason,
    provenance: ResolutionProvenance,
) -> PublicationResult {
    PublicationResult::Published {
        selected_source: Arc::clone(source),
        semantic_authority: SemanticAuthority::Resolved,
        exactness,
        reason: Box::new(reason),
        provenance: PublicationProvenance::Resolved { provenance },
    }
}

/// One row-owned authority/evidence/publication bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
#[serde(rename_all = "camelCase")]
pub struct TypePublication {
    authority: ResolvedTypeAuthority,
    evidence: Option<AuthoredTypeEvidence>,
    result: PublicationResult,
}

impl TypePublication {
    #[must_use]
    pub fn new(
        authority: ResolvedTypeAuthority,
        evidence: Option<AuthoredTypeEvidence>,
        policy: &PublicationPolicy,
    ) -> Self {
        let result = select_type_publication(&authority, evidence.as_ref(), policy);
        Self {
            authority,
            evidence,
            result,
        }
    }

    #[must_use]
    pub fn from_source_position(
        position: &SourcePosition,
        exactness: ResolutionExactness,
        provenance: ResolutionProvenance,
        diagnostics: Arc<[ResolutionDiagnostic]>,
        evidence: Option<AuthoredTypeEvidence>,
        policy: &PublicationPolicy,
    ) -> Self {
        Self::new(
            ResolvedTypeAuthority::from_source_position(
                position,
                exactness,
                provenance,
                diagnostics,
            ),
            evidence,
            policy,
        )
    }

    #[must_use]
    pub fn authority(&self) -> &ResolvedTypeAuthority {
        &self.authority
    }

    #[must_use]
    pub fn evidence(&self) -> Option<&AuthoredTypeEvidence> {
        self.evidence.as_ref()
    }

    #[must_use]
    pub fn result(&self) -> &PublicationResult {
        &self.result
    }

    pub fn select_with(&mut self, policy: &PublicationPolicy) {
        self.result = select_type_publication(&self.authority, self.evidence.as_ref(), policy);
    }

    #[must_use]
    pub fn source_position(&self) -> SourcePosition {
        self.result.source_position()
    }

    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self.result, PublicationResult::Failed { .. })
    }

    #[must_use]
    pub fn absolutized_against(&self, canonical_id: &str) -> Self {
        let authority = match &self.authority.outcome {
            ResolvedTypeOutcome::Present { source, exactness } => ResolvedTypeAuthority::present(
                source.absolutized_against(canonical_id),
                *exactness,
                self.authority.provenance,
                Arc::clone(&self.authority.diagnostics),
            ),
            ResolvedTypeOutcome::Absent { absence } => ResolvedTypeAuthority::absent(
                *absence,
                self.authority.provenance,
                Arc::clone(&self.authority.diagnostics),
            ),
            ResolvedTypeOutcome::Failed { failure } => ResolvedTypeAuthority::failed(
                *failure,
                self.authority.provenance,
                Arc::clone(&self.authority.diagnostics),
            ),
        };
        let evidence = self
            .evidence
            .as_ref()
            .map(|evidence| evidence.absolutized_against(canonical_id));
        let policy = match &self.result {
            PublicationResult::Published { reason, .. } => match reason.as_ref() {
                PublicationReason::AuthoredForIncomplete { policy } => policy.clone(),
                PublicationReason::AuthoredSymbolicRepresentation { proof } => {
                    PublicationPolicy::exact_only()
                        .with_symbolic_equivalence(proof.absolutized_against(canonical_id))
                }
                PublicationReason::ResolvedExactConcrete
                | PublicationReason::ResolvedExactSymbolic
                | PublicationReason::ResolvedIncomplete => PublicationPolicy::exact_only(),
            },
            PublicationResult::Absent { .. } | PublicationResult::Failed { .. } => {
                PublicationPolicy::exact_only()
            }
        };
        Self::new(authority, evidence, &policy)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
    use crate::locators::{
        AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    };
    use crate::{PrimitiveName, TopLevelOwnerId};

    use super::*;

    fn resolved_source() -> SemanticTypeSource {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::String,
        )))
    }

    fn locator(field_index: u32) -> MacroPayloadLocator {
        MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/ws/Comp.vue"),
                owner: TopLevelOwnerId::instance(0),
                symbol: Arc::from("defineProps"),
                space: LocatorSymbolSpace::Type,
            },
            macro_index: 0,
            payload: MacroPayloadPosition::Field { field_index },
        }
    }

    fn authority(exactness: ResolutionExactness) -> ResolvedTypeAuthority {
        ResolvedTypeAuthority::present(
            resolved_source(),
            exactness,
            ResolutionProvenance::SemanticEvaluator,
            Arc::from([]),
        )
    }

    fn evidence(field_index: u32) -> AuthoredTypeEvidence {
        // SAFETY: the unit fixture constructs one atomic producer row.
        let mint = unsafe { AuthoredSourceMint::new_unchecked() };
        AuthoredTypeEvidence::from_macro_payload(
            &mint,
            &locator(field_index),
            Arc::from("ImportedProps['value']"),
        )
    }

    #[test]
    fn exact_concrete_always_publishes_resolved_source() {
        let authority = authority(ResolutionExactness::ExactConcrete);
        let evidence = evidence(0);
        // SAFETY: selector adversary fixture binds its proof to the exact
        // authority/evidence pair. ExactConcrete must ignore even a matching
        // proof plus an incomplete-fallback permit.
        let mint = unsafe { SymbolicEquivalenceMint::new_unchecked() };
        let proof = SymbolicEquivalenceProof::from_lossless_projection(
            &mint,
            SymbolicEquivalenceKind::ImportedMacroCompound,
            resolved_source(),
            evidence.source().clone(),
        );
        let adversarial_policy = PublicationPolicy::allow_authored_for_incomplete(
            PublicationPolicyReason::ImportedMacroCompound,
        );
        let adversarial_policy = adversarial_policy.with_symbolic_equivalence(proof);
        let result = select_type_publication(&authority, Some(&evidence), &adversarial_policy);

        let PublicationResult::Published {
            selected_source,
            semantic_authority,
            exactness,
            reason,
            ..
        } = result
        else {
            panic!("exact concrete authority must publish");
        };
        assert_eq!(selected_source.as_ref(), &resolved_source());
        assert_eq!(semantic_authority, SemanticAuthority::Resolved);
        assert_eq!(exactness, ResolutionExactness::ExactConcrete);
        assert_eq!(reason.as_ref(), &PublicationReason::ResolvedExactConcrete);
    }

    #[test]
    fn exact_symbolic_authored_representation_requires_matching_typed_proof() {
        let authority = authority(ResolutionExactness::ExactSymbolic);
        let original_authority = authority.clone();
        let evidence = evidence(1);

        let without_proof = select_type_publication(
            &authority,
            Some(&evidence),
            &PublicationPolicy::exact_only(),
        );
        assert_eq!(
            without_proof.selected_source(),
            Some(&resolved_source()),
            "evidence alone is not a symbolic-equivalence proof"
        );

        // SAFETY: this selector unit binds the proof to its equal fixture
        // projections; session policy mismatch discrimination is tested in
        // `component_meta_resolution_policy_tests`.
        let mint = unsafe { SymbolicEquivalenceMint::new_unchecked() };
        let proof = SymbolicEquivalenceProof::from_lossless_projection(
            &mint,
            SymbolicEquivalenceKind::ImportedIndexedAccess,
            resolved_source(),
            evidence.source().clone(),
        );
        let policy = PublicationPolicy::exact_only().with_symbolic_equivalence(proof.clone());
        let with_proof = select_type_publication(&authority, Some(&evidence), &policy);

        let PublicationResult::Published {
            selected_source,
            semantic_authority,
            exactness,
            reason,
            ..
        } = with_proof
        else {
            panic!("exact symbolic authority must publish");
        };
        assert_eq!(
            selected_source.as_ref(),
            &evidence.source().to_semantic_source()
        );
        assert_eq!(semantic_authority, SemanticAuthority::Resolved);
        assert_eq!(exactness, ResolutionExactness::ExactSymbolic);
        assert_eq!(
            reason.as_ref(),
            &PublicationReason::AuthoredSymbolicRepresentation { proof }
        );
        assert_eq!(
            authority, original_authority,
            "representation selection must not overwrite resolved authority"
        );
    }

    #[test]
    fn incomplete_authored_fallback_requires_policy_and_stays_incomplete() {
        let authority = authority(ResolutionExactness::Incomplete);
        let evidence = evidence(2);

        let exact_only = select_type_publication(
            &authority,
            Some(&evidence),
            &PublicationPolicy::exact_only(),
        );
        assert_eq!(exact_only.selected_source(), Some(&resolved_source()));

        let policy = PublicationPolicy::allow_authored_for_incomplete(
            PublicationPolicyReason::ImportedMacroCompound,
        );
        let permitted = select_type_publication(&authority, Some(&evidence), &policy);
        let PublicationResult::Published {
            selected_source,
            semantic_authority,
            exactness,
            reason,
            ..
        } = permitted
        else {
            panic!("the incomplete resolved lane remains publishable");
        };
        assert_eq!(
            selected_source.as_ref(),
            &evidence.source().to_semantic_source()
        );
        assert_eq!(semantic_authority, SemanticAuthority::AuthoredFallback);
        assert_eq!(exactness, ResolutionExactness::Incomplete);
        assert_eq!(
            reason.as_ref(),
            &PublicationReason::AuthoredForIncomplete {
                policy: policy.clone()
            }
        );
    }

    #[test]
    fn failed_authority_is_absorbing_even_with_evidence_and_permit_policy() {
        let authority = ResolvedTypeAuthority::failed(
            TypedResolutionFailure::SourceConstruction(
                crate::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
            ),
            ResolutionProvenance::SemanticEvaluator,
            Arc::from([]),
        );
        let evidence = evidence(3);
        let policy = PublicationPolicy::allow_authored_for_incomplete(
            PublicationPolicyReason::ImportedMacroCompound,
        );

        assert!(matches!(
            select_type_publication(&authority, Some(&evidence), &policy),
            PublicationResult::Failed { .. }
        ));
    }

    #[test]
    fn selector_signature_excludes_terminal_display_input() {
        let _: fn(
            &ResolvedTypeAuthority,
            Option<&AuthoredTypeEvidence>,
            &PublicationPolicy,
        ) -> PublicationResult = select_type_publication;
    }
}
