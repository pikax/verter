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

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static MEMBER_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static COLLISION_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn note_member_visit() {
    #[cfg(any(test, feature = "test-support"))]
    MEMBER_VISITS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn note_collision_comparison() {
    #[cfg(any(test, feature = "test-support"))]
    COLLISION_COMPARISONS.with(|count| count.set(count.get() + 1));
}

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

    /// The selector this alternative answers.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn selector(&self) -> ObjectProjectionSelector {
        match &self.selected {
            SelectedProjection::Key(key) => ObjectProjectionSelector::Key(key.clone()),
            SelectedProjection::RelationShape(keys) => {
                ObjectProjectionSelector::RelationShape(Arc::clone(keys))
            }
            SelectedProjection::Surface => ObjectProjectionSelector::Surface,
            SelectedProjection::IndexDomain(domain) => {
                ObjectProjectionSelector::IndexDomain(*domain)
            }
            SelectedProjection::Signature(kind) => ObjectProjectionSelector::Signature(*kind),
            SelectedProjection::EnumerableValueEnvelope(domain) => {
                ObjectProjectionSelector::EnumerableValueEnvelope(*domain)
            }
            SelectedProjection::ExcessEligibility => ObjectProjectionSelector::ExcessEligibility,
        }
    }

    /// Visit every graph node the alternative names.
    pub(crate) fn for_each_retained_node(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        for fact in self.positive.iter() {
            if let ProjectionEvidence::Proven(node) = fact.value() {
                visit(*node);
            }
        }
        for index in self.indices.iter() {
            visit(index.key_type());
            if let ProjectionEvidence::Proven(node) = index.value() {
                visit(*node);
            }
        }
        for signature in self.signatures.iter() {
            visit(signature.node());
        }
        match &self.domain {
            AlternativeDomain::Closed(_) => {}
            AlternativeDomain::Open(open) => open.residual_operands.iter().copied().for_each(visit),
        }
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

//// Opaque correlated finite-union formula.
///
/// The alternatives are kept FACTORED (see
/// `project_semantic_dispatch::object_spread_product`): the formula owns the
/// sum of its spread layers, never their Cartesian product. A consumer reads
/// alternatives on demand through
/// `ProjectSemanticDispatch::object_spread_cursor`, which charges each one
/// to its connected demand before building it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProjectionFormula {
    product: verter_session_query::retention::owned::Owned<
        crate::project_semantic_dispatch::object_spread_product::SpreadProduct,
    >,
}

impl ObjectProjectionFormula {
    pub(crate) fn from_product(
        product: crate::project_semantic_dispatch::object_spread_product::SpreadProduct,
    ) -> Self {
        Self {
            product: verter_session_query::retention::owned::Owned::new(product),
        }
    }

    /// The product's owned allocation, which carries the formula's charge.
    pub(crate) fn product_owned(
        &self,
    ) -> &verter_session_query::retention::owned::Owned<
        crate::project_semantic_dispatch::object_spread_product::SpreadProduct,
    > {
        &self.product
    }

    pub(crate) fn product(
        &self,
    ) -> &crate::project_semantic_dispatch::object_spread_product::SpreadProduct {
        &self.product
    }

    pub(crate) fn for_each_retained_node(&self, mut visit: impl FnMut(SemanticNodeId)) {
        self.product.for_each_retained_node(&mut visit);
    }

    /// How many correlated alternatives the formula has — the size of the
    /// product, known without building one of them.
    #[must_use]
    pub fn width(&self) -> u64 {
        self.product.width()
    }

    /// Whether every alternative is closed (carries no residual operand).
    /// Decided on the factored product; whether an alternative's closed
    /// domain is WHOLE or selector-local is still that alternative's own
    /// witness to give.
    #[must_use]
    pub fn all_alternatives_closed(&self) -> bool {
        self.product.closed()
    }
}
// Exact lookup result available only on a closed alternative witness.
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
    pub(crate) fn to_closed_surface_view(self) -> Option<super::SurfaceView> {
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
                note_member_visit();
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

impl super::allocation_footprint::OwnedHeap for ObjectProjectionFormula {
    /// The product — an owned allocation charged by its own cell.
    fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        let Self { product } = self;
        product.offer(aliases)
    }
}

impl super::allocation_footprint::OwnedHeap for MemberFacets {
    /// The declaration origin's text; every other facet is inline.
    fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use super::allocation_footprint::copy_inline;
        let MemberFacets {
            readonly,
            method_kind,
            has_implementation_body,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        } = self;
        copy_inline(readonly)
            + copy_inline(method_kind)
            + copy_inline(has_implementation_body)
            + copy_inline(visibility)
            + copy_inline(spans)
            + declaration_origin.owned_heap_bytes(aliases)
            + copy_inline(declared_in_macro_type_arg)
            + copy_inline(merge_role)
            + copy_inline(excess_origin)
    }
}

impl super::allocation_footprint::OwnedHeap for ObjectProjectionIndex {
    /// The declaration origin's text; every other field is inline.
    fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use super::allocation_footprint::copy_inline;
        let ObjectProjectionIndex {
            domain,
            key_type,
            value,
            readonly,
            spans,
            declaration_origin,
        } = self;
        copy_inline(domain)
            + copy_inline(key_type)
            + match value {
                ProjectionEvidence::Proven(node) => copy_inline(node),
                ProjectionEvidence::Indeterminate => 0,
            }
            + match readonly {
                ProjectionEvidence::Proven(readonly) => copy_inline(readonly),
                ProjectionEvidence::Indeterminate => 0,
            }
            + copy_inline(spans)
            + declaration_origin.owned_heap_bytes(aliases)
    }
}

impl super::allocation_footprint::OwnedHeap for ObjectProjectionAlternative {
    /// Every fact, key, witness, index, signature and excess array with
    /// their key text and declaration origins. Proven values are graph
    /// ordinals. Destructured exhaustively: a new field fails to compile
    /// until its heap is counted.
    fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use super::allocation_footprint::{copy_inline, copy_slice_inline};
        use verter_session_query::retention::allocation::{arc_slice_block_bytes, AliasSet};
        fn fact(fact: &PositiveKeyFact, aliases: &mut AliasSet) -> usize {
            let PositiveKeyFact {
                key,
                presence,
                value,
                facets,
            } = fact;
            key.owned_heap_bytes(aliases)
                + copy_inline(presence)
                + match value {
                    ProjectionEvidence::Proven(node) => copy_inline(node),
                    ProjectionEvidence::Indeterminate => 0,
                }
                + match facets {
                    ProjectionEvidence::Proven(facets) => facets.owned_heap_bytes(aliases),
                    ProjectionEvidence::Indeterminate => 0,
                }
        }
        fn alternative_bytes(
            alternative: &ObjectProjectionAlternative,
            aliases: &mut AliasSet,
        ) -> usize {
            let ObjectProjectionAlternative {
                positive,
                selected,
                domain,
                indices,
                signatures,
                excess,
            } = alternative;
            let mut bytes = 0;
            if let Some(block) = arc_slice_block_bytes(positive, aliases) {
                bytes += block;
                for item in positive.iter() {
                    bytes += fact(item, aliases);
                }
            }
            bytes += match selected {
                SelectedProjection::Key(key) => key.owned_heap_bytes(aliases),
                SelectedProjection::RelationShape(shape) => shape.owned_heap_bytes(aliases),
                SelectedProjection::IndexDomain(domain)
                | SelectedProjection::EnumerableValueEnvelope(domain) => copy_inline(domain),
                SelectedProjection::Signature(kind) => copy_inline(kind),
                SelectedProjection::Surface | SelectedProjection::ExcessEligibility => 0,
            };
            bytes += match domain {
                AlternativeDomain::Closed(ClosedDomainData {
                    complete_keys,
                    scope,
                }) => complete_keys.owned_heap_bytes(aliases) + copy_inline(scope),
                AlternativeDomain::Open(OpenDomainWitnesses {
                    residual_operands,
                    indeterminate_possible_writes,
                }) => {
                    residual_operands.owned_heap_bytes(aliases)
                        + indeterminate_possible_writes.owned_heap_bytes(aliases)
                }
            };
            if let Some(block) = arc_slice_block_bytes(indices, aliases) {
                bytes += block;
                for item in indices.iter() {
                    bytes += item.owned_heap_bytes(aliases);
                }
            }
            bytes += arc_slice_block_bytes(signatures, aliases).unwrap_or(0)
                + copy_slice_inline(signatures);
            bytes += match excess {
                ExcessEligibility::Eligible { direct_candidates } => {
                    direct_candidates.owned_heap_bytes(aliases)
                }
                ExcessEligibility::SuppressedByGenericSpread | ExcessEligibility::Indeterminate => {
                    0
                }
            };
            bytes
        }
        alternative_bytes(self, aliases)
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

    /// A formula of finished alternatives, read back verbatim.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub(crate) fn formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        ObjectProjectionFormula::from_product(
            crate::project_semantic_dispatch::object_spread_product::SpreadProduct::sealed(
                alternatives.into_iter().collect(),
            ),
        )
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

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;

    /// Actual member copying/writes and property-identity comparisons on this thread.
    pub fn work_counts() -> (usize, usize) {
        (
            MEMBER_VISITS.with(std::cell::Cell::get),
            COLLISION_COMPARISONS.with(std::cell::Cell::get),
        )
    }

    #[must_use]
    pub fn context(
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
    pub fn positive_key(
        key: PropertyKey,
        presence: PositiveKeyPresence,
        value: ProjectionEvidence<SemanticNodeId>,
    ) -> PositiveKeyFact {
        evaluator_support::positive_key(key, presence, value, ProjectionEvidence::Indeterminate)
    }

    #[must_use]
    pub fn closed_alternative(
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

    /// The alternatives of a fixture formula, read back verbatim.
    #[must_use]
    pub fn sealed_alternatives(
        formula: &ObjectProjectionFormula,
    ) -> Vec<ObjectProjectionAlternative> {
        formula.product().sealed_alternatives()
    }

    #[must_use]
    pub fn closed_formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula(alternatives)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn formula(
        alternatives: impl IntoIterator<Item = ObjectProjectionAlternative>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula(alternatives)
    }

    #[must_use]
    #[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn open_formula(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
        residual_operands: impl IntoIterator<Item = SemanticNodeId>,
    ) -> ObjectProjectionFormula {
        evaluator_support::formula([open_alternative_with_possible_writes(
            positive,
            residual_operands,
            [],
        )])
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn open_alternative_with_possible_writes(
        positive: impl IntoIterator<Item = PositiveKeyFact>,
        residual_operands: impl IntoIterator<Item = SemanticNodeId>,
        indeterminate_possible_writes: impl IntoIterator<Item = PropertyKey>,
    ) -> ObjectProjectionAlternative {
        evaluator_support::alternative(evaluator_support::AlternativeInput {
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
        })
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
