//! Selector-aware, correlated object-spread projection facts.
//!
//! The formula deliberately keeps finite-union alternatives separate. An
//! ordinary alternative exposes positive evidence only; exact absence,
//! emptiness, `keyof`, exhaustive iteration, and closed materialisation require
//! one of the borrowed witnesses minted by [`ObjectProjectionAlternative::closed`]
//! or [`ObjectProjectionFormula::closed`].

use std::sync::Arc;

use super::{
    HashValue, ProjectionReductionContext, PropertyKey, SemanticNodeId, SubstitutionCanonicalHash,
};

/// The finite selector vocabulary of [`super::SemanticQueryKey::ProjectObjectSpread`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectProjectionSelector {
    Key(PropertyKey),
    RelationShape(Arc<[PropertyKey]>),
    Surface,
    IndexDomain(IndexDomain),
    Signature(ObjectSignatureKind),
    EnumerableValueEnvelope(IndexDomain),
    ExcessEligibility,
}

/// Property-key domain selected for index or enumerable-value facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexDomain {
    String,
    Number,
    Symbol,
}

/// Signature bucket selected from an object projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectSignatureKind {
    Call,
    Construct,
}

/// The exact-optional-property semantic policy carried on query identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExactOptionalPropertyPolicy {
    Disabled,
    Enabled,
}

/// Complete content-free context of an object-spread projection query.
///
/// Fields are private so callers cannot partially assemble the identity or
/// forget the exact-optional policy. Production construction is witness-gated
/// through `ProjectSemanticDispatch::object_spread_projection_context_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectSpreadProjectionContext {
    projection_reduction: ProjectionReductionContext,
    resolve_env_hash: HashValue,
    type_env_hash: HashValue,
    lib_env_hash: HashValue,
    project_identity: HashValue,
    substitution: SubstitutionCanonicalHash,
    optional_property_policy: ExactOptionalPropertyPolicy,
}

// Compile-bind the exhaustive R6 witness to the sealed context. The body uses
// a no-`..` destructure, so adding any identity field fails compilation until
// it is explicitly classified.
const _: fn(&ObjectSpreadProjectionContext) = w_object_spread_projection_context;

#[allow(dead_code)]
fn w_object_spread_projection_context(context: &ObjectSpreadProjectionContext) {
    let ObjectSpreadProjectionContext {
        projection_reduction,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
        optional_property_policy,
    } = context;
    super::w_projection_reduction_context(projection_reduction);
    object_spread_resolve_env_dim(resolve_env_hash);
    object_spread_type_env_dim(type_env_hash);
    object_spread_lib_env_dim(lib_env_hash);
    object_spread_project_identity_dim(project_identity);
    object_spread_substitution_dim(substitution);
    match optional_property_policy {
        ExactOptionalPropertyPolicy::Disabled | ExactOptionalPropertyPolicy::Enabled => {}
    }
}

#[allow(dead_code)]
fn object_spread_resolve_env_dim(_value: &HashValue) {}
#[allow(dead_code)]
fn object_spread_type_env_dim(_value: &HashValue) {}
#[allow(dead_code)]
fn object_spread_lib_env_dim(_value: &HashValue) {}
#[allow(dead_code)]
fn object_spread_project_identity_dim(_value: &HashValue) {}
#[allow(dead_code)]
fn object_spread_substitution_dim(_value: &SubstitutionCanonicalHash) {}

impl ObjectSpreadProjectionContext {
    #[must_use]
    pub(crate) const fn new(
        projection_reduction: ProjectionReductionContext,
        resolve_env_hash: HashValue,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
        project_identity: HashValue,
        substitution: SubstitutionCanonicalHash,
        optional_property_policy: ExactOptionalPropertyPolicy,
        _witness: crate::project_semantic_dispatch::ObjectSpreadProjectionContextWitness,
    ) -> Self {
        Self {
            projection_reduction,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
            optional_property_policy,
        }
    }

    #[must_use]
    pub const fn projection_reduction(self) -> ProjectionReductionContext {
        self.projection_reduction
    }

    #[must_use]
    pub const fn resolve_env_hash(self) -> HashValue {
        self.resolve_env_hash
    }

    #[must_use]
    pub const fn type_env_hash(self) -> HashValue {
        self.type_env_hash
    }

    #[must_use]
    pub const fn lib_env_hash(self) -> HashValue {
        self.lib_env_hash
    }

    #[must_use]
    pub const fn project_identity(self) -> HashValue {
        self.project_identity
    }

    #[must_use]
    pub const fn substitution(self) -> SubstitutionCanonicalHash {
        self.substitution
    }

    #[must_use]
    pub const fn optional_property_policy(self) -> ExactOptionalPropertyPolicy {
        self.optional_property_policy
    }
}

/// Whether a positively known property is required or optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositiveKeyPresence {
    Required,
    Optional,
}

/// Proof strength of one projected fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProjectionEvidence<T> {
    Proven(T),
    Indeterminate,
}

/// Member metadata kept alongside the projected value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberFacets {
    readonly: bool,
    method_kind: Option<verter_type_expr::ObjectMethodKind>,
    has_implementation_body: bool,
    visibility: verter_type_expr::MemberVisibility,
    spans: verter_type_expr::MemberSpans,
    declaration_origin: Option<Arc<str>>,
    declared_in_macro_type_arg: super::MacroOwnBodyStamp,
    merge_role: super::MergeRoleStamp,
    excess_origin: verter_type_expr::ExcessPropertyOrigin,
}

impl MemberFacets {
    #[must_use]
    pub const fn readonly(&self) -> bool {
        self.readonly
    }

    #[must_use]
    pub const fn method_kind(&self) -> Option<verter_type_expr::ObjectMethodKind> {
        self.method_kind
    }

    #[must_use]
    pub const fn visibility(&self) -> verter_type_expr::MemberVisibility {
        self.visibility
    }

    #[must_use]
    pub const fn has_implementation_body(&self) -> bool {
        self.has_implementation_body
    }

    #[must_use]
    pub const fn spans(&self) -> verter_type_expr::MemberSpans {
        self.spans
    }

    #[must_use]
    pub fn declaration_origin(&self) -> Option<&Arc<str>> {
        self.declaration_origin.as_ref()
    }

    #[must_use]
    pub const fn declared_in_macro_type_arg(&self) -> super::MacroOwnBodyStamp {
        self.declared_in_macro_type_arg
    }

    #[must_use]
    pub const fn merge_role(&self) -> super::MergeRoleStamp {
        self.merge_role
    }

    #[must_use]
    pub const fn excess_origin(&self) -> verter_type_expr::ExcessPropertyOrigin {
        self.excess_origin
    }
}

/// Positive, typed evidence for a named property.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PositiveKeyFact {
    key: PropertyKey,
    presence: PositiveKeyPresence,
    value: ProjectionEvidence<SemanticNodeId>,
    facets: ProjectionEvidence<MemberFacets>,
}

impl PositiveKeyFact {
    #[must_use]
    pub fn key(&self) -> &PropertyKey {
        &self.key
    }

    #[must_use]
    pub const fn presence(&self) -> PositiveKeyPresence {
        self.presence
    }

    #[must_use]
    pub const fn value(&self) -> &ProjectionEvidence<SemanticNodeId> {
        &self.value
    }

    #[must_use]
    pub const fn facets(&self) -> &ProjectionEvidence<MemberFacets> {
        &self.facets
    }
}

/// One known index-domain fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectProjectionIndex {
    domain: IndexDomain,
    key_type: SemanticNodeId,
    value: ProjectionEvidence<SemanticNodeId>,
    readonly: ProjectionEvidence<bool>,
    spans: verter_type_expr::IndexSignatureSpans,
    declaration_origin: Option<Arc<str>>,
}

impl ObjectProjectionIndex {
    #[must_use]
    pub const fn domain(&self) -> IndexDomain {
        self.domain
    }

    #[must_use]
    pub const fn value(&self) -> &ProjectionEvidence<SemanticNodeId> {
        &self.value
    }

    #[must_use]
    pub const fn key_type(&self) -> SemanticNodeId {
        self.key_type
    }

    #[must_use]
    pub const fn readonly(&self) -> &ProjectionEvidence<bool> {
        &self.readonly
    }

    #[must_use]
    pub const fn spans(&self) -> verter_type_expr::IndexSignatureSpans {
        self.spans
    }

    #[must_use]
    pub fn declaration_origin(&self) -> Option<&Arc<str>> {
        self.declaration_origin.as_ref()
    }
}

/// One known call/construct signature fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectProjectionSignature {
    kind: ObjectSignatureKind,
    node: SemanticNodeId,
}

impl ObjectProjectionSignature {
    #[must_use]
    pub const fn kind(self) -> ObjectSignatureKind {
        self.kind
    }

    #[must_use]
    pub const fn node(self) -> SemanticNodeId {
        self.node
    }
}

/// Typed excess-property eligibility evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExcessEligibility {
    Eligible {
        direct_candidates: Arc<[PropertyKey]>,
    },
    SuppressedByGenericSpread,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClosedDomainData {
    complete_keys: Arc<[PropertyKey]>,
    scope: ClosedDomainScope,
}

/// What a CLOSED alternative domain actually proves.
///
/// A construction program evaluated under a selector-filtered demand
/// (`Key(k)`, `RelationShape(keys)`, index/signature selectors) produces
/// alternatives whose positive facts cover ONLY the selector's declared
/// key set — minting `AlternativeDomain::Closed` whenever
/// `residual_operands` is empty would let a `Key("x")` formula "prove"
/// the absence of `y`, and selector liveness (which prunes earlier
/// spread effects a later direct write dominates) would record no
/// residual at all. The scope records which witness the domain IS:
///
/// - [`ClosedDomainScope::Whole`] — the evaluation covered the whole
///   program (`Surface` / `ExcessEligibility` selectors, which never
///   prune effects) and no residual remains: the complete key domain is
///   exact, so `keyof` / emptiness / complete-domain iteration / whole
///   surface materialisation are available.
/// - [`ClosedDomainScope::SelectorLocal`] — the evaluation covered only
///   the selector's declared keys. Within that declared set,
///   presence/absence is proven (the liveness dominance rule: a pruned
///   spread cannot affect a dominated selected key, and a
///   `RelationShape` declares its whole consulted key set); domain-wide
///   operations stay sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClosedDomainScope {
    Whole,
    SelectorLocal,
}

impl ClosedDomainScope {
    /// Derive the scope from the demand's selector. `Surface` and
    /// `ExcessEligibility` evaluate every effect (`selector_live_start`
    /// only prunes for `Key`), so their closed domains are whole; every
    /// other selector's closed domain is local to its declared keys.
    fn for_selector(selector: &ObjectProjectionSelector) -> Self {
        match selector {
            ObjectProjectionSelector::Surface | ObjectProjectionSelector::ExcessEligibility => {
                Self::Whole
            }
            _ => Self::SelectorLocal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenDomainWitnesses {
    residual_operands: Arc<[SemanticNodeId]>,
    indeterminate_possible_writes: Arc<[PropertyKey]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AlternativeDomain {
    Closed(ClosedDomainData),
    Open(OpenDomainWitnesses),
}

/// Private selector result carried by an alternative. It prevents one
/// selector's result from being reinterpreted as another selector's payload.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectedProjection {
    Key(PropertyKey),
    RelationShape(Arc<[PropertyKey]>),
    Surface,
    IndexDomain(IndexDomain),
    Signature(ObjectSignatureKind),
    EnumerableValueEnvelope(IndexDomain),
    ExcessEligibility,
}

impl From<&ObjectProjectionSelector> for SelectedProjection {
    fn from(selector: &ObjectProjectionSelector) -> Self {
        match selector {
            ObjectProjectionSelector::Key(key) => Self::Key(key.clone()),
            ObjectProjectionSelector::RelationShape(keys) => Self::RelationShape(Arc::clone(keys)),
            ObjectProjectionSelector::Surface => Self::Surface,
            ObjectProjectionSelector::IndexDomain(domain) => Self::IndexDomain(*domain),
            ObjectProjectionSelector::Signature(kind) => Self::Signature(*kind),
            ObjectProjectionSelector::EnumerableValueEnvelope(domain) => {
                Self::EnumerableValueEnvelope(*domain)
            }
            ObjectProjectionSelector::ExcessEligibility => Self::ExcessEligibility,
        }
    }
}

/// One correlated alternative in an object projection formula.
///
/// Construction and domain data are private. The ordinary public surface
/// exposes positive facts and open-safe selected-key evidence only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProjectionAlternative {
    positive: Arc<[PositiveKeyFact]>,
    selected: SelectedProjection,
    domain: AlternativeDomain,
    indices: Arc<[ObjectProjectionIndex]>,
    signatures: Arc<[ObjectProjectionSignature]>,
    excess: ExcessEligibility,
}

impl ObjectProjectionAlternative {
    /// Borrow the positive facts. Omission is not absence evidence.
    #[must_use]
    pub const fn positive(&self) -> PositiveAlternativeEvidence<'_> {
        PositiveAlternativeEvidence { alternative: self }
    }

    /// Read a key without ever turning omission from an open alternative into
    /// absence. JS property identity: either authored spelling finds the
    /// colliding fact (`Number(1)` and `String("1")` are one property).
    #[must_use]
    pub fn selected_key(&self, key: &PropertyKey) -> OpenSafeKeyEvidence<'_> {
        if let Some(fact) = self
            .positive
            .iter()
            .find(|fact| fact.key.element_access_collides(key))
        {
            return OpenSafeKeyEvidence::Positive(fact);
        }
        match &self.domain {
            AlternativeDomain::Open(open)
                if open
                    .indeterminate_possible_writes
                    .iter()
                    .any(|candidate| candidate.element_access_collides(key)) =>
            {
                OpenSafeKeyEvidence::IndeterminatePossibleWrite
            }
            AlternativeDomain::Open(open) => OpenSafeKeyEvidence::UnknownOnOpenDomain {
                residual_operands: &open.residual_operands,
            },
            // An ordinary alternative is intentionally not an absence-proof
            // surface. Callers that need exact absence must obtain `closed()`
            // and use `ClosedObjectProjectionAlternative::lookup`.
            AlternativeDomain::Closed(_) => OpenSafeKeyEvidence::UnknownOnOpenDomain {
                residual_operands: &[],
            },
        }
    }

    /// Obtain the unforgeable borrowed complete-domain witness.
    #[must_use]
    pub const fn closed(&self) -> Option<ClosedObjectProjectionAlternative<'_>> {
        match self.domain {
            AlternativeDomain::Closed(_) => {
                Some(ClosedObjectProjectionAlternative { alternative: self })
            }
            AlternativeDomain::Open(_) => None,
        }
    }

    /// Selector-local call/construct facts. These remain readable on an open
    /// named-key domain because spread operands never copy signatures.
    #[must_use]
    pub fn signatures(&self) -> &[ObjectProjectionSignature] {
        &self.signatures
    }

    /// Selector-local index facts. Named-key openness does not turn a known
    /// index signature into named-property presence.
    #[must_use]
    pub fn indices(&self) -> &[ObjectProjectionIndex] {
        &self.indices
    }

    /// Whole-program excess eligibility. A semantic generic spread suppresses
    /// freshness even when the named-key domain remains open.
    #[must_use]
    pub const fn excess(&self) -> &ExcessEligibility {
        &self.excess
    }
}

/// Positive-only view of an alternative.
#[derive(Clone, Copy)]
pub struct PositiveAlternativeEvidence<'a> {
    alternative: &'a ObjectProjectionAlternative,
}

impl<'a> PositiveAlternativeEvidence<'a> {
    #[must_use]
    pub fn get(self, key: &PropertyKey) -> Option<&'a PositiveKeyFact> {
        self.alternative
            .positive
            .iter()
            .find(|fact| fact.key.element_access_collides(key))
    }

    /// Visit positive facts. This is deliberately not an exhaustive-domain
    /// iterator: omission from the callback stream proves nothing.
    pub fn visit(self, mut visitor: impl FnMut(&'a PositiveKeyFact)) {
        self.alternative.positive.iter().for_each(&mut visitor);
    }
}

/// Key evidence available without a complete-domain witness.
///
/// There is intentionally no `Absent` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenSafeKeyEvidence<'a> {
    Positive(&'a PositiveKeyFact),
    IndeterminatePossibleWrite,
    UnknownOnOpenDomain {
        residual_operands: &'a [SemanticNodeId],
    },
}

/// Opaque correlated finite-union formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProjectionFormula {
    alternatives: Arc<[ObjectProjectionAlternative]>,
    closed_keyof: Option<Arc<[PropertyKey]>>,
}

impl ObjectProjectionFormula {
    #[must_use]
    pub fn alternatives(&self) -> &[ObjectProjectionAlternative] {
        &self.alternatives
    }

    /// Obtain a formula-wide complete-domain witness only when every
    /// alternative is closed.
    #[must_use]
    pub const fn closed(&self) -> Option<ClosedObjectProjectionFormula<'_>> {
        match self.closed_keyof {
            Some(_) => Some(ClosedObjectProjectionFormula { formula: self }),
            None => None,
        }
    }

    #[allow(dead_code)] // Evaluator-owned constructor; unit builders reuse it.
    fn new(alternatives: Arc<[ObjectProjectionAlternative]>) -> Self {
        verter_debug_assert!(
            alternatives.first().is_none_or(|first| alternatives
                .iter()
                .all(|alternative| alternative.selected == first.selected)),
            "all alternatives in one projection formula must carry the same selector"
        );
        let closed_keyof = if alternatives
            .iter()
            .all(|alternative| matches!(alternative.domain, AlternativeDomain::Closed(_)))
        {
            // Formula-wide exact keyof: keys present in EVERY alternative.
            // Matching uses element-access collision (the engine's JS
            // property identity model: `{1: x}` and `{"1": x}` are one
            // property), keeping the FIRST alternative's spelling for
            // stability. DELIBERATE divergence from tsc, whose type-level
            // `keyof (A | B)` intersects nominally (`1 & "1"` is never):
            // the rest of this engine (fold, lookups, relation) already
            // treats dual spellings as one property, so the formula
            // agrees with the engine, not with tsc's nominal rule.
            let mut common: Vec<PropertyKey> = alternatives
                .first()
                .map(|alternative| match &alternative.domain {
                    AlternativeDomain::Closed(closed) => closed.complete_keys.to_vec(),
                    AlternativeDomain::Open(_) => unreachable!("all alternatives checked closed"),
                })
                .unwrap_or_default();
            common.retain(|key| {
                alternatives.iter().skip(1).all(|alternative| {
                    matches!(
                        &alternative.domain,
                        AlternativeDomain::Closed(closed)
                            if closed.complete_keys.iter().any(|candidate| candidate.element_access_collides(key))
                    )
                })
            });
            Some(Arc::from(common))
        } else {
            None
        };
        Self {
            alternatives,
            closed_keyof,
        }
    }
}

/// Exact lookup result available only on a closed alternative witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedKeyLookup<'a> {
    Present(&'a PositiveKeyFact),
    AbsentProven,
}

mod complete_domain_seal {
    pub trait Sealed {}
}

/// Marker implemented only by unforgeable borrowed complete-domain witnesses.
pub trait CompleteObjectDomain: complete_domain_seal::Sealed {}

/// Borrowed proof that one alternative's key domain is complete.
#[derive(Clone, Copy)]
pub struct ClosedObjectProjectionAlternative<'a> {
    alternative: &'a ObjectProjectionAlternative,
}

impl complete_domain_seal::Sealed for ClosedObjectProjectionAlternative<'_> {}
impl CompleteObjectDomain for ClosedObjectProjectionAlternative<'_> {}

impl<'a> ClosedObjectProjectionAlternative<'a> {
    fn domain(self) -> &'a ClosedDomainData {
        match &self.alternative.domain {
            AlternativeDomain::Closed(closed) => closed,
            AlternativeDomain::Open(_) => {
                unreachable!("closed witness can only be minted from a closed domain")
            }
        }
    }

    /// Whether this closed domain covers the WHOLE program (see
    /// [`ClosedDomainScope`]) — the gate for every domain-wide operation.
    fn is_whole_domain(self) -> bool {
        self.domain().scope == ClosedDomainScope::Whole
    }

    /// Whether `key` is inside the selector's declared key set — the set
    /// a selector-local closed domain proves presence/absence for (the
    /// liveness dominance rule). Uses element-access collision: a
    /// `Key(Number(1))` selector declares the `String("1")` spelling.
    fn declares(self, key: &PropertyKey) -> bool {
        match &self.alternative.selected {
            SelectedProjection::Key(selected) => selected.element_access_collides(key),
            SelectedProjection::RelationShape(keys) => keys
                .iter()
                .any(|selected| selected.element_access_collides(key)),
            _ => false,
        }
    }

    /// The complete key domain — WHOLE-domain only: a selector-filtered
    /// closed alternative's positives are the selected keys, not the
    /// program's domain.
    #[must_use]
    pub fn complete_keys(self) -> Option<&'a [PropertyKey]> {
        self.is_whole_domain()
            .then(|| &*self.domain().complete_keys)
    }

    #[must_use]
    pub fn exact_key_domain(self) -> Option<&'a [PropertyKey]> {
        self.complete_keys()
    }

    /// Key evidence under the closed domain. Returns `None` when the
    /// closed domain cannot speak for `key`: a selector-local closed
    /// alternative only proves presence/absence WITHIN the selector's
    /// declared key set (see [`ClosedDomainScope`]); absence of any other
    /// key is NOT proven, so there is no answer rather than a forged
    /// `AbsentProven`.
    #[must_use]
    pub fn lookup(self, key: &PropertyKey) -> Option<ClosedKeyLookup<'a>> {
        if !self.is_whole_domain() && !self.declares(key) {
            return None;
        }
        Some(
            self.alternative
                .positive
                .iter()
                .find(|fact| fact.key.element_access_collides(key))
                .map_or(ClosedKeyLookup::AbsentProven, ClosedKeyLookup::Present),
        )
    }

    /// Domain emptiness — WHOLE-domain only.
    #[must_use]
    pub fn is_empty(self) -> Option<bool> {
        self.is_whole_domain()
            .then(|| self.domain().complete_keys.is_empty())
    }

    #[must_use]
    pub fn keyof(self) -> Option<&'a [PropertyKey]> {
        self.complete_keys()
    }

    #[must_use]
    pub fn exact_keyof(self) -> Option<&'a [PropertyKey]> {
        self.keyof()
    }

    /// Exhaustively iterate the complete named-key domain — WHOLE-domain
    /// only (a selector-filtered alternative's positives are not the
    /// complete domain).
    pub fn iter(self) -> Option<impl ExactSizeIterator<Item = &'a PositiveKeyFact>> {
        self.is_whole_domain()
            .then(|| self.alternative.positive.iter())
    }

    pub fn iterate_exhaustively(
        self,
    ) -> Option<impl ExactSizeIterator<Item = &'a PositiveKeyFact>> {
        self.iter()
    }

    /// Materialise the exact closed branch surface — WHOLE-domain only.
    #[must_use]
    pub fn surface(self) -> Option<ClosedObjectProjectionSurface<'a>> {
        self.is_whole_domain()
            .then_some(ClosedObjectProjectionSurface {
                alternative: self.alternative,
            })
    }

    #[must_use]
    pub fn to_closed_surface_view(self) -> Option<super::SurfaceView> {
        // A complete surface materialisation is a WHOLE-domain operation
        // (see [`ClosedDomainScope`]).
        if !self.is_whole_domain() {
            return None;
        }
        let members = self
            .alternative
            .positive
            .iter()
            .map(|fact| {
                let ProjectionEvidence::Proven(value) = &fact.value else {
                    return None;
                };
                let ProjectionEvidence::Proven(facets) = &fact.facets else {
                    return None;
                };
                Some(super::SurfaceMember {
                    key: super::AuthoredPropertyKey::from_known(fact.key.clone()),
                    value: *value,
                    optional: fact.presence == PositiveKeyPresence::Optional,
                    readonly: facets.readonly,
                    method_kind: facets.method_kind,
                    has_implementation_body: facets.has_implementation_body,
                    visibility: facets.visibility,
                    spans: facets.spans,
                    declaration_origin: facets.declaration_origin.clone(),
                    declared_in_macro_type_arg: facets.declared_in_macro_type_arg,
                    merge_role: facets.merge_role,
                    excess_origin: facets.excess_origin,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut call_signatures = Vec::new();
        let mut construct_signatures = Vec::new();
        for signature in self.alternative.signatures.iter() {
            match signature.kind {
                ObjectSignatureKind::Call => call_signatures.push(signature.node),
                ObjectSignatureKind::Construct => construct_signatures.push(signature.node),
            }
        }
        let index_signatures = self
            .alternative
            .indices
            .iter()
            .map(|index| {
                let ProjectionEvidence::Proven(value_type) = &index.value else {
                    return None;
                };
                let ProjectionEvidence::Proven(readonly) = &index.readonly else {
                    return None;
                };
                Some(super::IndexSignature {
                    key_type: index.key_type,
                    value_type: *value_type,
                    readonly: *readonly,
                    spans: index.spans,
                    declaration_origin: index.declaration_origin.clone(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let has_index_signature = !index_signatures.is_empty();
        Some(super::SurfaceView::new(
            Arc::from(members),
            Arc::from(call_signatures),
            Arc::from(construct_signatures),
            Arc::from(index_signatures),
            None,
            // Truthful positive evidence (matching `lower` /
            // `locator_shape` / `surface_view_from_shallow`): a non-empty
            // index list IS an index signature — a hardcoded `false`
            // would let the raise fold's single-call fast path fire and
            // silently drop the carried signatures.
            has_index_signature,
        ))
    }
}

/// Borrowed exact materialisation of one closed alternative.
#[derive(Clone, Copy)]
pub struct ClosedObjectProjectionSurface<'a> {
    alternative: &'a ObjectProjectionAlternative,
}

impl<'a> ClosedObjectProjectionSurface<'a> {
    #[must_use]
    pub fn members(self) -> &'a [PositiveKeyFact] {
        &self.alternative.positive
    }

    #[must_use]
    pub fn indices(self) -> &'a [ObjectProjectionIndex] {
        &self.alternative.indices
    }

    #[must_use]
    pub fn signatures(self) -> &'a [ObjectProjectionSignature] {
        &self.alternative.signatures
    }

    #[must_use]
    pub const fn excess(self) -> &'a ExcessEligibility {
        &self.alternative.excess
    }
}

/// Borrowed proof that every alternative in a formula is closed.
#[derive(Clone, Copy)]
pub struct ClosedObjectProjectionFormula<'a> {
    formula: &'a ObjectProjectionFormula,
}

impl complete_domain_seal::Sealed for ClosedObjectProjectionFormula<'_> {}
impl CompleteObjectDomain for ClosedObjectProjectionFormula<'_> {}

impl<'a> ClosedObjectProjectionFormula<'a> {
    /// Exhaustively iterate correlated alternatives without flattening them.
    pub fn alternatives(
        self,
    ) -> impl ExactSizeIterator<Item = ClosedObjectProjectionAlternative<'a>> {
        self.formula
            .alternatives
            .iter()
            .map(|alternative| ClosedObjectProjectionAlternative { alternative })
    }

    /// Formula-wide exact keyof — WHOLE-domain only: `None` when any
    /// alternative's closed domain is selector-local (see
    /// [`ClosedDomainScope`]).
    #[must_use]
    pub fn keyof(self) -> Option<&'a [PropertyKey]> {
        let whole = self.formula.alternatives.iter().all(|alternative| {
            matches!(
                alternative.domain,
                AlternativeDomain::Closed(ClosedDomainData {
                    scope: ClosedDomainScope::Whole,
                    ..
                })
            )
        });
        whole.then(|| {
            self.formula
                .closed_keyof
                .as_deref()
                .expect("closed witness requires formula-wide exact keyof")
        })
    }

    #[must_use]
    pub fn exact_keyof(self) -> Option<&'a [PropertyKey]> {
        self.keyof()
    }

    /// Formula emptiness — WHOLE-domain only.
    #[must_use]
    pub fn is_empty(self) -> Option<bool> {
        let whole = self.formula.alternatives.iter().all(|alternative| {
            matches!(
                alternative.domain,
                AlternativeDomain::Closed(ClosedDomainData {
                    scope: ClosedDomainScope::Whole,
                    ..
                })
            )
        });
        whole.then(|| {
            self.formula
                .alternatives
                .iter()
                .all(|alternative| alternative.positive.is_empty())
        })
    }

    /// Exhaustively materialise each correlated closed surface —
    /// WHOLE-domain alternatives only (`None` per selector-local
    /// alternative; see [`ClosedDomainScope`]).
    pub fn surfaces(
        self,
    ) -> impl ExactSizeIterator<Item = Option<ClosedObjectProjectionSurface<'a>>> {
        self.alternatives().map(|alternative| alternative.surface())
    }

    pub fn to_closed_surface_views(
        self,
    ) -> impl ExactSizeIterator<Item = Option<super::SurfaceView>> + 'a {
        self.alternatives()
            .map(ClosedObjectProjectionAlternative::to_closed_surface_view)
    }

    pub fn iterate_exhaustively(
        self,
    ) -> impl ExactSizeIterator<Item = ClosedObjectProjectionAlternative<'a>> {
        self.alternatives()
    }
}

/// Evaluator-owned constructors. The module is crate-private in production;
/// unit tests reuse the same invariant-checking builders through `test_support`.
#[allow(dead_code)] // Production consumer lands with the ordered-effect evaluator.
pub(crate) mod evaluator_support {
    use super::*;

    #[derive(Debug, Clone)]
    pub(crate) struct AlternativeInput {
        pub(crate) positive: Arc<[PositiveKeyFact]>,
        pub(crate) selector: ObjectProjectionSelector,
        pub(crate) closed: bool,
        pub(crate) residual_operands: Arc<[SemanticNodeId]>,
        pub(crate) indeterminate_possible_writes: Arc<[PropertyKey]>,
        pub(crate) indices: Arc<[ObjectProjectionIndex]>,
        pub(crate) signatures: Arc<[ObjectProjectionSignature]>,
        pub(crate) excess: ExcessEligibility,
    }

    #[must_use]
    pub(crate) fn alternative(input: AlternativeInput) -> ObjectProjectionAlternative {
        let complete_keys: Arc<[PropertyKey]> = Arc::from(
            input
                .positive
                .iter()
                .map(|fact| fact.key.clone())
                .collect::<Vec<_>>(),
        );
        verter_debug_assert_eq!(
            complete_keys
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            complete_keys.len(),
            "object projection alternatives must have one positive fact per key"
        );
        let domain = if input.closed {
            verter_debug_assert!(input.residual_operands.is_empty());
            verter_debug_assert!(input.indeterminate_possible_writes.is_empty());
            AlternativeDomain::Closed(ClosedDomainData {
                complete_keys,
                scope: ClosedDomainScope::for_selector(&input.selector),
            })
        } else {
            AlternativeDomain::Open(OpenDomainWitnesses {
                residual_operands: input.residual_operands,
                indeterminate_possible_writes: input.indeterminate_possible_writes,
            })
        };
        ObjectProjectionAlternative {
            positive: input.positive,
            selected: SelectedProjection::from(&input.selector),
            domain,
            indices: input.indices,
            signatures: input.signatures,
            excess: input.excess,
        }
    }

    #[must_use]
    pub(crate) fn formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        ObjectProjectionFormula::new(Arc::from(alternatives.into_iter().collect::<Vec<_>>()))
    }

    #[must_use]
    pub(crate) fn positive_key(
        key: PropertyKey,
        presence: PositiveKeyPresence,
        value: ProjectionEvidence<SemanticNodeId>,
        facets: ProjectionEvidence<MemberFacets>,
    ) -> PositiveKeyFact {
        PositiveKeyFact {
            key,
            presence,
            value,
            facets,
        }
    }

    #[must_use]
    pub(crate) fn member_facets(
        readonly: bool,
        method_kind: Option<verter_type_expr::ObjectMethodKind>,
        has_implementation_body: bool,
        visibility: verter_type_expr::MemberVisibility,
        spans: verter_type_expr::MemberSpans,
        declaration_origin: Option<Arc<str>>,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp,
        merge_role: crate::semantic_query::MergeRoleStamp,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
    ) -> MemberFacets {
        MemberFacets {
            readonly,
            method_kind,
            has_implementation_body,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        }
    }

    #[must_use]
    pub(crate) fn index(
        domain: IndexDomain,
        key_type: SemanticNodeId,
        value: ProjectionEvidence<SemanticNodeId>,
        readonly: ProjectionEvidence<bool>,
        spans: verter_type_expr::IndexSignatureSpans,
        declaration_origin: Option<Arc<str>>,
    ) -> ObjectProjectionIndex {
        ObjectProjectionIndex {
            domain,
            key_type,
            value,
            readonly,
            spans,
            declaration_origin,
        }
    }

    #[must_use]
    pub(crate) fn signature(
        kind: ObjectSignatureKind,
        node: SemanticNodeId,
    ) -> ObjectProjectionSignature {
        ObjectProjectionSignature { kind, node }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    #[must_use]
    pub(crate) fn context(
        projection_reduction: ProjectionReductionContext,
        resolve_env_hash: HashValue,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
        project_identity: HashValue,
        substitution: SubstitutionCanonicalHash,
        optional_property_policy: ExactOptionalPropertyPolicy,
    ) -> ObjectSpreadProjectionContext {
        ObjectSpreadProjectionContext {
            projection_reduction,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
            optional_property_policy,
        }
    }

    #[must_use]
    pub(crate) fn positive_key(
        key: PropertyKey,
        presence: PositiveKeyPresence,
        value: ProjectionEvidence<SemanticNodeId>,
    ) -> PositiveKeyFact {
        evaluator_support::positive_key(key, presence, value, ProjectionEvidence::Indeterminate)
    }

    #[must_use]
    pub(crate) fn closed_alternative(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
    ) -> ObjectProjectionAlternative {
        evaluator_support::alternative(evaluator_support::AlternativeInput {
            positive: Arc::from(positive.into_iter().collect::<Vec<_>>()),
            selector: ObjectProjectionSelector::Surface,
            closed: true,
            residual_operands: Arc::from([]),
            indeterminate_possible_writes: Arc::from([]),
            indices: Arc::from([]),
            signatures: Arc::from([]),
            excess: ExcessEligibility::Eligible {
                direct_candidates: Arc::from([]),
            },
        })
    }

    #[must_use]
    pub(crate) fn closed_formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula(alternatives)
    }

    #[must_use]
    pub(crate) fn formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula(alternatives)
    }

    #[must_use]
    pub(crate) fn open_alternative(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
        residual_operands: impl IntoIterator<Item = SemanticNodeId>,
    ) -> ObjectProjectionAlternative {
        evaluator_support::alternative(evaluator_support::AlternativeInput {
            positive: Arc::from(positive.into_iter().collect::<Vec<_>>()),
            selector: ObjectProjectionSelector::Surface,
            closed: false,
            residual_operands: Arc::from(residual_operands.into_iter().collect::<Vec<_>>()),
            indeterminate_possible_writes: Arc::from([]),
            indices: Arc::from([]),
            signatures: Arc::from([]),
            excess: ExcessEligibility::SuppressedByGenericSpread,
        })
    }

    #[must_use]
    pub(crate) fn open_formula(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
        residual_operands: impl IntoIterator<Item = SemanticNodeId>,
    ) -> ObjectProjectionFormula {
        open_formula_with_possible_writes(positive, residual_operands, [])
    }

    #[must_use]
    pub(crate) fn open_formula_with_possible_writes(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
        residual_operands: impl IntoIterator<Item = SemanticNodeId>,
        indeterminate_possible_writes: impl IntoIterator<Item = PropertyKey>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula([evaluator_support::alternative(
            evaluator_support::AlternativeInput {
                positive: Arc::from(positive.into_iter().collect::<Vec<_>>()),
                selector: ObjectProjectionSelector::Surface,
                closed: false,
                residual_operands: Arc::from(residual_operands.into_iter().collect::<Vec<_>>()),
                indeterminate_possible_writes: Arc::from(
                    indeterminate_possible_writes
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
                indices: Arc::from([]),
                signatures: Arc::from([]),
                excess: ExcessEligibility::SuppressedByGenericSpread,
            },
        )])
    }

    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn query(
        program: SemanticNodeId,
        selector: ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) -> super::super::SemanticQueryKey {
        super::super::SemanticQueryKey::ProjectObjectSpread {
            program,
            selector,
            context,
        }
    }
}
