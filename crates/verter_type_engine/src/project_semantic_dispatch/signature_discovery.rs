//! Graph-facing signature discovery: `SignaturesOfType` and
//! `ReadSignatureResult`.
//!
//! The subject walk settles a type to its signature-bearing shape (carriers
//! and aliases through the shared settlement rail, unions through the
//! `VerterStableV1` arm order, intersections in authored order, a
//! declaration's heritage body own signatures first, constrained
//! type parameters through their constraint, apparent primitives and
//! collections through the resolved global population) and publishes the
//! candidates through the record-level discovery in
//! [`crate::signature_kernel`]. Enumeration never reads a body: a body
//! return is a closed recipe until [`ProjectSemanticDispatch::read_signature_result`]
//! forces it.

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use verter_type_expr::OwnedStr;

use rustc_hash::{FxHashSet, FxHasher};

use crate::semantic_query::composite::CompositeOriginCategory;
use crate::semantic_query::{
    CanonicalTypeSubstitution, IncompleteReason, LiteralValue, ProjectionReductionContext,
    QueryOutcome, Ready, ResolveOverloadSetConsumer, ResultEvaluationContextId, SemanticContextId,
    SemanticNodeData, SemanticNodeId, SignatureKind as GraphSignatureKind, CONTEXT_FREE_EVALUATION,
    CONTEXT_FREE_EVIDENCE,
};
use crate::signature_kernel::{
    heritage_signatures, intersection_signatures, merged_declaration_signatures, publish_signature,
    set_from_candidates, union_signatures, AppliedResult, AppliedResultId, BinderInput,
    DeclarationGroupId, DeclarationParentId, DiscoveryError, DiscoveryTypes, EpochBinding,
    EpochRebind, ParamInput, RestInput, ResultDemand, ResultInput, SemanticReadView,
    SignatureCandidate, SignatureDescriptorId, SignatureInput, SignatureKind, SignatureProvenance,
    SignatureResultRecipe, SignatureSemanticFlags, SignatureStore, SlotTypeFacts, SourceLocatorId,
    TypeToken,
};
use verter_session_query::type_solver::arena::PrimitiveKind;

use super::ProjectSemanticDispatch;

/// Whether an intersection of `category` is an intersection TYPE — one the
/// checker reads by the intersection rules — rather than a declaration's
/// heritage or an overload group, which are signature lists of one symbol.
fn intersection_type_category(category: CompositeOriginCategory) -> bool {
    match category {
        CompositeOriginCategory::Heritage
        | CompositeOriginCategory::OverloadGroup
        | CompositeOriginCategory::MergedOverloadGroup => false,
        CompositeOriginCategory::Canonical(_)
        | CompositeOriginCategory::CanonicalUnproven
        | CompositeOriginCategory::AuthoredShell
        | CompositeOriginCategory::OrderedCarrier
        | CompositeOriginCategory::PreservingRebuild
        | CompositeOriginCategory::QuerySubject => true,
        #[cfg(any(test, feature = "test-support"))]
        CompositeOriginCategory::TestFixture => true,
    }
}

fn kernel_kind(kind: GraphSignatureKind) -> SignatureKind {
    match kind {
        GraphSignatureKind::Call => SignatureKind::Call,
        GraphSignatureKind::Construct => SignatureKind::Construct,
    }
}

fn hash_u64<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Full-width logical identity: two independently seeded 64-bit hashes.
fn identity_u128<T: Hash>(value: &T) -> u128 {
    let mut second = FxHasher::default();
    0x9E37_79B9_7F4A_7C15_u64.hash(&mut second);
    value.hash(&mut second);
    (u128::from(hash_u64(value)) << 64) | u128::from(second.finish())
}

/// `VerterStableV1`-independent, order-independent binder-space key: derived
/// from logical identity, never from intern order.
///
/// The key keeps 31 bits of the hash. An FxHash mixes its input's
/// high-order bytes poorly into its low bits, so identities that differ only
/// there (the stable keys of `(x: 249) => void` and `(x: 511) => void`, whose
/// number literals differ in the high bytes of an `f64`) agree on them far
/// more often than chance; the finalizer spreads every hashed bit over the
/// bits the key keeps. The finalizer is SplitMix64's output mix (Steele,
/// Lea and Flood, "Fast Splittable Pseudorandom Number Generators",
/// OOPSLA 2014; shift/multiply constants from Stafford's "Mix13" variant of
/// the MurmurHash3 `fmix64` finalizer).
fn space_key_of<T: Hash>(value: &T) -> u64 {
    let mut mixed = hash_u64(value);
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    mixed & 0x7FFF_FFFF
}

/// The binder space of a signature with no authored occurrence, named by
/// its complete stable key: every field derives from the exact key bytes,
/// never from the 64-bit fingerprint alone, so two signatures whose keys
/// share a fingerprint but differ in bytes claim two identities, and a
/// truncated-key clash between them is the store's typed collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RootlessSpace {
    key: u64,
    identity: u128,
    group: u64,
}

impl RootlessSpace {
    fn of(key: &crate::semantic_query::stable_key::StableKey) -> Self {
        let exact = key.exact();
        Self {
            key: space_key_of(&exact),
            identity: identity_u128(&exact),
            group: hash_u64(&exact),
        }
    }
}

/// Whether a canonical file id names a JavaScript-family module.
fn is_js_canonical(canonical_id: &str) -> bool {
    std::path::Path::new(canonical_id)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "js" | "jsx" | "mjs" | "cjs"
            )
        })
}

/// The graph adapter the record-level discovery reads types through, over
/// the one epoch its computation is bound to.
pub(crate) struct GraphTypes<'a, 'd, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'a ProjectSemanticDispatch<'d, C>,
    binding: &'a EpochBinding,
    semantic_identity: bool,
}

impl<'a, 'd, C: crate::resolver_core::ResolverCapabilities> GraphTypes<'a, 'd, C> {
    fn node(&self, token: TypeToken) -> Option<SemanticNodeId> {
        self.binding.view().type_token_node(token).ok()
    }

    fn token(&self, node: SemanticNodeId) -> Option<TypeToken> {
        self.binding.intern_type_token(node, None).ok()
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> SlotTypeFacts for GraphTypes<'_, '_, C> {
    fn accepts_void(&self, ty: TypeToken) -> bool {
        let Some(node) = self.node(ty) else {
            return false;
        };
        let graph = self.dispatch.graph();
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Primitive(PrimitiveKind::Void)) => true,
            Some(SemanticNodeData::Union(members)) => members.iter().any(|m| {
                matches!(
                    graph.node_data(*m).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Void))
                )
            }),
            _ => false,
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> DiscoveryTypes for GraphTypes<'_, '_, C> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        self.dispatch.graph().record_signature_position_compared();
        let na = self.node(a)?;
        let nb = self.node(b)?;
        let moving: Vec<(SemanticNodeId, SemanticNodeId)> =
            corr.iter().copied().filter(|(x, y)| x != y).collect();
        let na = if moving.is_empty() {
            na
        } else {
            self.dispatch.substitute_canonical(
                na,
                &CanonicalTypeSubstitution::new(self.dispatch.graph().store(), moving),
            )
        };
        if self.semantic_identity {
            return self.dispatch.relate_type_identity(na, nb);
        }
        if na == nb {
            return Some(true);
        }
        let graph = self.dispatch.graph();
        let ka = crate::semantic_query::stable_key::stable_key_for_node(graph.store(), na);
        let kb = crate::semantic_query::stable_key::stable_key_for_node(graph.store(), nb);
        Some(ka == kb)
    }

    fn intersect(&self, members: &[TypeToken]) -> Option<TypeToken> {
        let nodes: Option<Vec<SemanticNodeId>> = members.iter().map(|t| self.node(*t)).collect();
        let node = self
            .dispatch
            .intern_normalized_union_or_intersection(&nodes?, false);
        self.token(node)
    }

    fn map_binders(
        &self,
        ty: TypeToken,
        map: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<TypeToken> {
        let node = self.node(ty)?;
        let mapped = self.dispatch.substitute_canonical(
            node,
            &CanonicalTypeSubstitution::new(self.dispatch.graph().store(), map.to_vec()),
        );
        self.token(mapped)
    }

    fn unknown(&self) -> Option<TypeToken> {
        let node = self
            .dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        self.token(node)
    }

    fn any(&self) -> Option<TypeToken> {
        let node = self
            .dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
        self.token(node)
    }

    fn is_any(&self, ty: TypeToken) -> bool {
        self.node(ty).is_some_and(|node| {
            matches!(
                self.dispatch.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
            )
        })
    }

    fn forced_result(
        &self,
        candidate: &SignatureCandidate,
    ) -> Result<crate::signature_kernel::ForcedResult, IncompleteReason> {
        let empty = self
            .binding
            .intern_substitution(
                crate::signature_kernel::CallSubstitution::identity(
                    self.residual_space(candidate.signature)
                        .ok_or(IncompleteReason::UnsettledInput)?,
                ),
                None,
            )
            .map_err(|_| IncompleteReason::UnsettledInput)?;
        let result = self.dispatch.read_signature_result(
            self.binding,
            candidate.signature,
            empty,
            ResultDemand::Both,
            CONTEXT_FREE_EVALUATION,
            SemanticContextId::production(),
        );
        match result {
            QueryOutcome::Ready(Ready { value, .. }) => {
                let applied = self
                    .binding
                    .view()
                    .applied_result(value)
                    .map_err(|_| IncompleteReason::UnsettledInput)?;
                Ok(crate::signature_kernel::ForcedResult {
                    return_type: applied
                        .return_type
                        .ok_or(IncompleteReason::UnsettledInput)?,
                    effects: applied.effects,
                })
            }
            QueryOutcome::Incomplete(reason) => Err(reason),
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> GraphTypes<'_, '_, C> {
    fn residual_space(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Option<crate::signature_kernel::BinderSpaceId> {
        self.binding
            .view()
            .descriptor(descriptor)
            .ok()
            .map(|d| d.residual_binders)
    }
}

type Found = Result<Vec<SignatureCandidate>, DiscoveryError>;

fn unsettled<T>() -> Result<T, DiscoveryError> {
    Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))
}

fn unsupported<T>() -> Result<T, DiscoveryError> {
    Err(DiscoveryError::Incomplete(IncompleteReason::Unsupported))
}

struct Walk<'w, 'a, 'd, C: crate::resolver_core::ResolverCapabilities> {
    types: &'w GraphTypes<'a, 'd, C>,
    kind: GraphSignatureKind,
    context: SemanticContextId,
    visiting: FxHashSet<SemanticNodeId>,
    /// The authored node each leaf candidate was published from, first
    /// publication wins within this walk.
    authored: rustc_hash::FxHashMap<SignatureDescriptorId, SemanticNodeId>,
}

impl<'w, 'a, 'd, C: crate::resolver_core::ResolverCapabilities> Walk<'w, 'a, 'd, C> {
    fn dispatch(&self) -> &'a ProjectSemanticDispatch<'d, C> {
        self.types.dispatch
    }

    fn strict_null(&self) -> bool {
        self.context
            .context()
            .effective_semantic_options
            .strict_null_checks
    }

    /// The members of an intersection type with every member that is itself
    /// an intersection type — written, or named by an alias or a value's
    /// type — spread in place, as the checker's `getIntersectionType`
    /// flattens them: `A & (B & K)` has the members `A`, `B` and `K`, so the
    /// mixin rule reads each constructor (`typeof M1(M0(Base))` mixes both
    /// layers into `Base`'s construct signature). Iterative, so an
    /// intersection nested N deep is N member reads.
    fn flattened_intersection_members(&self, members: Vec<SemanticNodeId>) -> Vec<SemanticNodeId> {
        let graph = self.dispatch().graph();
        let mut flattened = Vec::with_capacity(members.len());
        let mut seen = FxHashSet::default();
        let mut pending: Vec<SemanticNodeId> = members.into_iter().rev().collect();
        while let Some(member) = pending.pop() {
            if !seen.insert(member) {
                continue;
            }
            let mut settled = self.dispatch().resolve_signature_source_carrier(
                member,
                ProjectionReductionContext::structural_transit(),
            );
            let mut aliases = FxHashSet::default();
            while let Some(SemanticNodeData::Alias(target)) = graph.node_data(settled).as_deref() {
                if !aliases.insert(*target) {
                    break;
                }
                settled = *target;
            }
            match graph.node_data(settled).as_deref() {
                Some(SemanticNodeData::Intersection(nested))
                    if intersection_type_category(nested.origin_category()) =>
                {
                    pending.extend(nested.iter().rev().copied());
                }
                _ => flattened.push(member),
            }
        }
        flattened
    }

    fn discover(&mut self, node: SemanticNodeId) -> Found {
        self.dispatch().graph().record_discovery_node_walked();
        // A lib runtime nominal's builtin-sentinel carrier is TERMINAL: it
        // names a global interface, not a declaration with a body, so the
        // global population answers for it BEFORE the settlement rail —
        // which would resolve it to its unresolvable-declaration miss.
        if let Some(found) = self.runtime_nominal_carrier(node) {
            return found;
        }
        let node = self.dispatch().resolve_signature_source_carrier(
            node,
            ProjectionReductionContext::structural_transit(),
        );
        if !self.visiting.insert(node) {
            return unsettled();
        }
        let out = self.discover_settled(node);
        self.visiting.remove(&node);
        out
    }

    fn discover_settled(&mut self, node: SemanticNodeId) -> Found {
        let graph = self.dispatch().graph();
        let Some(data) = graph.node_data(node) else {
            return unsettled();
        };
        match &*data {
            SemanticNodeData::Alias(target) => {
                let target = *target;
                drop(data);
                self.discover(target)
            }
            // A class expression's INSTANCE carries the signatures of its
            // instance surface.
            SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                let surface = *surface;
                drop(data);
                let surface = self
                    .dispatch()
                    .class_expression_read_surface(node)
                    .unwrap_or(surface);
                self.discover(surface)
            }
            SemanticNodeData::MergedDecl { contributors } => {
                let contributors = Arc::clone(contributors.as_arc());
                drop(data);
                self.discover_merged_declaration(&contributors)
            }
            SemanticNodeData::Signature { .. } | SemanticNodeData::DeferredCallable(_) => {
                drop(data);
                Ok(self.leaf(node, 0)?.into_iter().collect())
            }
            SemanticNodeData::Object(surface) => {
                let list = match self.kind {
                    GraphSignatureKind::Call => Arc::clone(&surface.call_signatures),
                    GraphSignatureKind::Construct => Arc::clone(&surface.construct_signatures),
                };
                drop(data);
                self.signature_list(&list, 0)
            }
            SemanticNodeData::TypeParam { constraint, .. } => {
                let constraint = *constraint;
                drop(data);
                match constraint {
                    Some(c) => self.discover(c),
                    None => Ok(Vec::new()),
                }
            }
            SemanticNodeData::Union(_) => {
                drop(data);
                let arms = crate::semantic_query::stable_key::semantic_union_members(
                    graph.store(),
                    node,
                    self.context.context(),
                );
                let mut lists = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    lists.push(self.discover(*arm)?);
                }
                union_signatures(self.types.binding, self.types, &lists)
            }
            SemanticNodeData::Intersection(members) => {
                let category = members.origin_category();
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                let members = if intersection_type_category(category) {
                    self.flattened_intersection_members(members)
                } else {
                    members
                };
                let mut lists = Vec::with_capacity(members.len());
                for member in members {
                    lists.push(self.discover(member)?);
                }
                match category {
                    // An interface or class body with heritage is a
                    // declaration, not an intersection type: it inherits
                    // its bases' signatures by concatenation, own first.
                    CompositeOriginCategory::Heritage => Ok(heritage_signatures(&lists)),
                    // One method's overloads are its signature list, every
                    // declaration's kept.
                    CompositeOriginCategory::OverloadGroup => Ok(lists.concat()),
                    CompositeOriginCategory::MergedOverloadGroup => {
                        merged_declaration_signatures(self.types.binding, &lists)
                    }
                    CompositeOriginCategory::Canonical(_)
                    | CompositeOriginCategory::CanonicalUnproven
                    | CompositeOriginCategory::AuthoredShell
                    | CompositeOriginCategory::OrderedCarrier
                    | CompositeOriginCategory::PreservingRebuild
                    | CompositeOriginCategory::QuerySubject => intersection_signatures(
                        self.types.binding,
                        self.types,
                        kernel_kind(self.kind),
                        &lists,
                    ),
                    #[cfg(any(test, feature = "test-support"))]
                    CompositeOriginCategory::TestFixture => intersection_signatures(
                        self.types.binding,
                        self.types,
                        kernel_kind(self.kind),
                        &lists,
                    ),
                }
            }
            SemanticNodeData::Primitive(primitive) => {
                let name = match primitive {
                    PrimitiveKind::String => Some("String"),
                    PrimitiveKind::Number => Some("Number"),
                    PrimitiveKind::Boolean => Some("Boolean"),
                    PrimitiveKind::Symbol => Some("Symbol"),
                    PrimitiveKind::BigInt => Some("BigInt"),
                    PrimitiveKind::Any
                    | PrimitiveKind::Unknown
                    | PrimitiveKind::Void
                    | PrimitiveKind::Never
                    | PrimitiveKind::Null
                    | PrimitiveKind::Undefined
                    | PrimitiveKind::Object => None,
                };
                drop(data);
                match name {
                    Some(name) => self.apparent(name, &[]),
                    None => Ok(Vec::new()),
                }
            }
            SemanticNodeData::Literal(value) => {
                let name = match value {
                    LiteralValue::String(_) => "String",
                    LiteralValue::Number(_) => "Number",
                    LiteralValue::Boolean(_) => "Boolean",
                    LiteralValue::BigInt(_) => "BigInt",
                };
                drop(data);
                self.apparent(name, &[])
            }
            SemanticNodeData::TemplateLiteral { .. } => {
                drop(data);
                self.apparent("String", &[])
            }
            // An enum member's apparent type is its value's.
            SemanticNodeData::EnumLiteral(literal) => {
                let base = literal.base;
                drop(data);
                self.discover(base)
            }
            SemanticNodeData::TypeOfNominal(_) => {
                drop(data);
                self.apparent("Symbol", &[])
            }
            SemanticNodeData::Array { element, readonly } => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                self.apparent(if readonly { "ReadonlyArray" } else { "Array" }, &[element])
            }
            SemanticNodeData::Tuple { readonly, .. } => {
                let readonly = *readonly;
                drop(data);
                let unknown =
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
                self.apparent(if readonly { "ReadonlyArray" } else { "Array" }, &[unknown])
            }
            SemanticNodeData::KeyOf { .. } | SemanticNodeData::Mapped { .. } => Ok(Vec::new()),
            // The checker's error type reads as `any`, which has no signatures.
            SemanticNodeData::Opaque(error) if super::build::is_certified_recovery(error) => {
                Ok(Vec::new())
            }
            SemanticNodeData::Opaque(error) => Err(DiscoveryError::Incomplete(
                IncompleteReason::from_query_error(error),
            )),
            SemanticNodeData::ObjectSpreadProgram(_)
            | SemanticNodeData::SyntheticBinding { .. } => unsupported(),
            SemanticNodeData::IntrinsicApplication { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::RawFallback { .. } => unsettled(),
        }
    }

    /// [`Self::runtime_nominal`] over `node` when it IS a lib runtime
    /// nominal's builtin-sentinel carrier, `None` for every other node. The
    /// nominal's type ARGUMENTS travel with it: a generic augmentation
    /// (`interface Promise<T> { (value: T): void }`) is instantiated with
    /// them.
    fn runtime_nominal_carrier(&mut self, node: SemanticNodeId) -> Option<Found> {
        let d = self.dispatch();
        let nominal = match d.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::DeclRef { identity })
                if d.runtime_nominal_identity(identity).is_some() =>
            {
                (identity.decl_name.clone(), Vec::new())
            }
            Some(SemanticNodeData::InstantiationRef { base, args })
                if d.runtime_nominal_identity(base).is_some() =>
            {
                (base.decl_name.clone(), args.iter().copied().collect())
            }
            _ => return None,
        };
        Some(self.runtime_nominal(&nominal.0, &nominal.1))
    }

    /// The signatures of a lib runtime nominal interface applied to
    /// `type_arguments`.
    ///
    /// These are OPEN interfaces: the pristine lib declaration of `Function`
    /// / `Date` / `Promise` declares no call signature, but a project's
    /// `declare global` block can merge some in. The contributors come from
    /// the shared augmentation folder, which also observes the augmenter-set
    /// fingerprint so a later `declare global` invalidates the answer; only
    /// with no contributor is the set empty. Each contributor's own
    /// signatures are discovered through THIS walk, so an augmenter's
    /// intersection / alias / overload shape is read exactly once, by the
    /// one authority. A torn contributor is served but folds the no-warm
    /// rail, exactly as the external augmentation path does.
    fn runtime_nominal(&mut self, name: &str, type_arguments: &[SemanticNodeId]) -> Found {
        let d = self.dispatch();
        // Contributor discovery for `declare global` is the ingestion-time
        // population: a program-root `.d.ts` that nothing imports is
        // recorded when its artifact publishes, so lookup does not scan
        // program membership. Compiler options (noLib / lib) come from the
        // request's owning project, not the first workspace file.
        let request_canonical = crate::request_context::current_request_canonical();
        let Some(contributions) = d.collect_augmentation_contributions(
            super::build::AugmentationContribution::TypeBody,
            verter_session_query::resolution::AugmentationTargetKind::GlobalAugmentation,
            name,
            type_arguments,
            ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Expanded),
            request_canonical.as_deref().unwrap_or(""),
            None,
        ) else {
            return Ok(Vec::new());
        };
        if contributions.source_env_unobservable {
            d.fold_into_top_build_local_taint(false, true);
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnobservableSource,
            );
        }
        let mut out = Vec::new();
        for contributor in contributions.contributor_nodes {
            out.extend(self.discover(contributor)?);
        }
        Ok(out)
    }

    /// Apparent-type signatures of a global wrapper interface: the wrapper
    /// the demanding project declares
    /// ([`ProjectSemanticDispatch::global_wrapper_surface`]). An absent
    /// global (`noLib`) is the checker's empty apparent type: a complete
    /// negative, which a declaration appearing later invalidates.
    ///
    /// A read of a primitive subject arrives with its key already rewritten
    /// to that wrapper
    /// (the signature stage of its entry's admission, `query_entry`); one
    /// reached here — a union arm, an unscoped or wrapper-less read — depends
    /// on the demanding project, which its key does not name, so the answer
    /// stays out of the shared memo with every entry that reads it.
    fn apparent(&mut self, name: &str, args: &[SemanticNodeId]) -> Found {
        let d = self.dispatch();
        let Some(canonical) = d.wrapper_demand_canonical() else {
            return unsettled();
        };
        d.fold_into_top_build_local_taint(false, true);
        match d.global_wrapper_surface(name, args, canonical.as_ref()) {
            super::apparent_type::GlobalWrapper::Surface(surface) => self.discover(surface),
            super::apparent_type::GlobalWrapper::Absent => Ok(Vec::new()),
            super::apparent_type::GlobalWrapper::Unsettled => unsettled(),
        }
    }

    /// The candidates of an ordered list of signature nodes — one object's
    /// call or construct list — whose ordinals start at `first_ordinal`.
    fn signature_list(&mut self, list: &[SemanticNodeId], first_ordinal: u32) -> Found {
        let mut out = Vec::with_capacity(list.len());
        for (offset, sig) in list.iter().enumerate() {
            let sig = self.dispatch().resolve_signature_source_carrier(
                *sig,
                ProjectionReductionContext::structural_transit(),
            );
            match self.leaf(sig, first_ordinal + offset as u32)? {
                Some(candidate) => out.push(candidate),
                None => return unsupported(),
            }
        }
        Ok(out)
    }

    /// The signatures of a merged declaration: every contributor's own
    /// signatures in contributor order — one symbol, each contributor its
    /// own declaration ([`merged_declaration_signatures`]) — then its
    /// `extends` bases in clause order, each base once (the heritage rule
    /// over the peer-merged body `reduce_merged_decl_with_graph` builds).
    fn discover_merged_declaration(&mut self, contributors: &[SemanticNodeId]) -> Found {
        let graph = self.dispatch().graph();
        let mut own: Vec<Vec<SemanticNodeId>> = Vec::with_capacity(contributors.len());
        let mut bases: Vec<SemanticNodeId> = Vec::new();
        let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
        for contributor in contributors {
            let mut surfaces = Vec::new();
            super::walk::collect_merged_contributor_arms(
                graph.store(),
                *contributor,
                &mut surfaces,
                &mut bases,
            );
            own.push(
                surfaces
                    .iter()
                    .flat_map(|surface| match self.kind {
                        GraphSignatureKind::Call => surface.call_signatures.iter(),
                        GraphSignatureKind::Construct => surface.construct_signatures.iter(),
                    })
                    .copied()
                    .filter(|signature| seen.insert(*signature))
                    .collect(),
            );
        }
        let mut lists = Vec::with_capacity(own.len());
        let mut ordinal = 0u32;
        for signatures in &own {
            lists.push(self.signature_list(signatures, ordinal)?);
            ordinal += signatures.len() as u32;
        }
        let mut out = merged_declaration_signatures(self.types.binding, &lists)?;
        for base in bases {
            out.extend(self.discover(base)?);
        }
        Ok(out)
    }

    /// One authored signature node as a published candidate, or `None` when
    /// the node is not a signature at all.
    fn leaf(
        &mut self,
        node: SemanticNodeId,
        ordinal: u32,
    ) -> Result<Option<SignatureCandidate>, DiscoveryError> {
        let graph = self.dispatch().graph();
        let Some(data) = graph.node_data(node) else {
            return unsettled();
        };
        let (kind, params, type_parameters, occurrence, carrier, span, predicate) = match &*data {
            SemanticNodeData::Signature {
                kind,
                params,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                predicate,
                ..
            } => (
                *kind,
                Arc::clone(params.as_arc()),
                Arc::clone(type_parameters.as_arc()),
                occurrence.clone(),
                return_carrier.clone(),
                *signature_span,
                *predicate,
            ),
            SemanticNodeData::DeferredCallable(callable) => {
                let parts = callable.parts(&ResolveOverloadSetConsumer::witness());
                (
                    parts.kind,
                    Arc::clone(parts.params),
                    Arc::clone(parts.type_parameters),
                    parts.occurrence.cloned(),
                    parts.return_carrier.clone(),
                    parts.signature_span,
                    parts.predicate,
                )
            }
            _ => return Ok(None),
        };
        drop(data);
        if kind != self.kind {
            return Ok(None);
        }
        let strict = self.strict_null();
        let (receiver, positional) = crate::semantic_query::split_this_receiver(&params);
        let to_param = |p: &crate::semantic_query::FunctionParam| ParamInput {
            name: p.name.as_deref().map(Arc::<str>::from),
            ty: p.ty,
            optional: p.optional,
            includes_undefined: p.optional && strict,
        };
        let mut fixed: Vec<ParamInput> = Vec::new();
        let mut rest: Option<RestInput> = None;
        for param in positional {
            if !param.rest {
                fixed.push(to_param(param));
                continue;
            }
            let settled = match self
                .dispatch()
                .unwrap_identity_carrier_for_relation(param.ty)
            {
                super::relation::IdentityCarrierUnwrap::Concrete(n) => Some(n),
                super::relation::IdentityCarrierUnwrap::Unresolvable => None,
            };
            let shape = settled.and_then(|n| graph.node_data(n));
            match shape.as_deref() {
                Some(SemanticNodeData::Array { element, .. }) => {
                    rest = Some(RestInput {
                        name: param.name.as_deref().map(Arc::<str>::from),
                        ty: *element,
                        generic: false,
                        tail: Vec::new(),
                    });
                }
                Some(SemanticNodeData::Tuple { elements, .. }) => {
                    let mut tail = Vec::new();
                    let mut run: Option<SemanticNodeId> = None;
                    for element in elements.iter() {
                        let optional = element.optional;
                        let as_param = ParamInput {
                            name: element.label.as_deref().map(Arc::<str>::from),
                            ty: element.value,
                            optional,
                            includes_undefined: optional && strict,
                        };
                        if element.rest {
                            let inner = match graph.node_data(element.value).as_deref() {
                                Some(SemanticNodeData::Array { element, .. }) => *element,
                                _ => element.value,
                            };
                            run = Some(inner);
                        } else if run.is_some() {
                            tail.push(as_param);
                        } else {
                            fixed.push(as_param);
                        }
                    }
                    if let Some(element) = run {
                        rest = Some(RestInput {
                            name: param.name.as_deref().map(Arc::<str>::from),
                            ty: element,
                            generic: false,
                            tail,
                        });
                    }
                }
                _ => {
                    rest = Some(RestInput {
                        name: param.name.as_deref().map(Arc::<str>::from),
                        ty: param.ty,
                        generic: true,
                        tail: Vec::new(),
                    });
                }
            }
        }
        let binders: Vec<BinderInput> = type_parameters
            .iter()
            .map(|decl| BinderInput {
                name: Arc::<str>::from(&decl.name),
                param: decl.param,
                constraint: decl.constraint,
                default: decl.default,
            })
            .collect();
        let result = match carrier.result_source() {
            crate::semantic_query::SignatureResultSource::Declared(return_type) => {
                ResultInput::Declared {
                    return_type: *return_type,
                    predicate_or_assertion: predicate,
                }
            }
            crate::semantic_query::SignatureResultSource::Function(source) => ResultInput::Body {
                locator: hash_u64(source),
            },
        };
        let (space_key, space_identity, group, parent, source_ordinal) = match &occurrence {
            Some(occ) => (
                space_key_of(&(&occ.function, occ.signature_ordinal)),
                identity_u128(&(&occ.function, occ.signature_ordinal)),
                hash_u64(&occ.function),
                hash_u64(&occ.function.anchor.canonical_id),
                occ.signature_ordinal,
            ),
            None => {
                let key =
                    crate::semantic_query::stable_key::stable_key_for_node(graph.store(), node);
                let space = RootlessSpace::of(&key);
                (space.key, space.identity, space.group, 0, ordinal)
            }
        };
        // An authored JavaScript signature with no type information at all:
        // every parameter is implicitly `any` and the result is inferred.
        let untyped_js = occurrence.as_ref().is_some_and(|occ| {
            is_js_canonical(&occ.function.anchor.canonical_id)
                && matches!(
                    carrier,
                    crate::semantic_query::SignatureReturnCarrier::Function(_)
                        | crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                        | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_)
                )
        }) && params.iter().all(|p| {
            matches!(
                graph.node_data(p.ty).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
            )
        });
        // TypeScript's `HasLiteralTypes`: a parameter (the `this`
        // receiver included) whose DECLARED type is a literal type makes
        // the signature a specialized one, which call resolution tries
        // first. A declaration fact, kept through instantiation.
        let specialized = params.iter().any(|param| param.declared_literal);
        let locator = span.map_or(0, |s| hash_u64(&(s.start, s.end)));
        let input = SignatureInput {
            kind: kernel_kind(kind),
            source: node,
            space_key,
            space_identity,
            binders,
            receiver: receiver.map(to_param),
            params: fixed,
            rest,
            flags: match (untyped_js, specialized) {
                (true, true) => SignatureSemanticFlags::UNTYPED_JS
                    .union(SignatureSemanticFlags::LITERAL_SPECIALIZATION),
                (true, false) => SignatureSemanticFlags::UNTYPED_JS,
                (false, true) => SignatureSemanticFlags::LITERAL_SPECIALIZATION,
                (false, false) => SignatureSemanticFlags::NONE,
            },
            result,
            provenance: SignatureProvenance::authored(
                DeclarationGroupId::from_raw(group),
                DeclarationParentId::from_raw(parent),
                source_ordinal,
                ordinal,
                SourceLocatorId::from_raw(locator),
            ),
        };
        let candidate = publish_signature(self.types.binding, &input)?;
        self.authored.entry(candidate.signature).or_insert(node);
        Ok(Some(candidate))
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// `SignaturesOfType(subject, kind, context)`: the ordered call or
    /// construct candidates of `subject`. An empty set is a complete
    /// negative; every failure to settle the subject is an explicit
    /// incomplete reason. The set alone; production reads
    /// [`Self::signatures_of_type_with_authored`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn signatures_of_type(
        &self,
        store: &SignatureStore,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> QueryOutcome<crate::signature_kernel::SignatureSetRef> {
        match self.signatures_of_type_with_authored(store, subject, kind, context) {
            QueryOutcome::Ready(Ready { value, evidence }) => QueryOutcome::Ready(Ready {
                value: value.set,
                evidence,
            }),
            QueryOutcome::Incomplete(reason) => QueryOutcome::Incomplete(reason),
        }
    }

    /// [`Self::signatures_of_type`] with, per candidate, the authored node
    /// this walk published it from.
    ///
    /// The walk works in the epoch current as it begins, for its whole life:
    /// everything it interns lands there. When a replacement retired that
    /// epoch before the set is published, the found candidates are copied
    /// into the current epoch and published there, never walked again.
    pub fn signatures_of_type_with_authored(
        &self,
        store: &SignatureStore,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> QueryOutcome<crate::signature_kernel::SignatureSetValue> {
        let binding = store.bind_current();
        let types = GraphTypes {
            dispatch: self,
            binding: &binding,
            semantic_identity: false,
        };
        let mut walk = Walk {
            types: &types,
            kind,
            context,
            visiting: FxHashSet::default(),
            authored: rustc_hash::FxHashMap::default(),
        };
        let found = walk.discover(subject);
        #[cfg(any(test, feature = "test-support"))]
        signatures_of_type_build_point(SignaturesOfTypeBuildPoint::Discovered);
        let published = found.and_then(|list| {
            let view = binding.view();
            let mut nodes_of: Vec<crate::signature_kernel::SignatureCandidateNodes> =
                Vec::with_capacity(list.len());
            for candidate in &list {
                let nodes = match composite_sequence(view, candidate.signature)? {
                    Some(sequence) => sequence
                        .edges
                        .iter()
                        .map(|edge| walk.authored.get(&edge.declaration).copied())
                        .collect::<Option<Vec<_>>>()
                        .unwrap_or_default(),
                    None => Vec::new(),
                };
                nodes_of.push(crate::signature_kernel::SignatureCandidateNodes {
                    authored: walk.authored.get(&candidate.signature).copied(),
                    constituents: verter_type_expr::OwnedSlice::from(nodes.into_boxed_slice()),
                });
            }
            let (set, version) = if binding.epoch() == store.epoch() {
                (
                    set_from_candidates(&binding, list)?,
                    crate::signature_kernel::SignatureVersion(Some(binding.lease())),
                )
            } else {
                self.publish_rebound_set(store, &binding, list)?
            };
            Ok((set, nodes_of, version))
        });
        match published {
            Ok((set, nodes_of, version)) => QueryOutcome::Ready(Ready {
                value: crate::signature_kernel::SignatureSetValue::new(
                    self.graph().store(),
                    version,
                    set,
                    nodes_of,
                ),
                evidence: CONTEXT_FREE_EVIDENCE,
            }),
            Err(error) => QueryOutcome::Incomplete(error.incomplete_reason()),
        }
    }

    /// The candidates a walk found in `binding`'s epoch, which a replacement
    /// retired before they were published: one pass copies them into the
    /// current epoch, whose set and lease the value then carries.
    #[cold]
    #[inline(never)]
    fn publish_rebound_set(
        &self,
        store: &SignatureStore,
        binding: &EpochBinding,
        list: Vec<SignatureCandidate>,
    ) -> Result<
        (
            crate::signature_kernel::SignatureSetRef,
            crate::signature_kernel::SignatureVersion,
        ),
        DiscoveryError,
    > {
        let into = store.bind_current();
        let mut rebind = EpochRebind::new(&binding.lease(), &into);
        let mut rebound = Vec::with_capacity(list.len());
        for candidate in list {
            rebound.push(rebind.candidate(candidate)?);
        }
        let copied = rebind.descriptors_copied();
        drop(rebind);
        let set = set_from_candidates(&into, rebound)?;
        self.graph().record_signature_rebind(copied);
        Ok((
            set,
            crate::signature_kernel::SignatureVersion(Some(into.lease())),
        ))
    }

    /// `ReadSignatureResult(descriptor, call substitution, demand,
    /// evaluation, context)`: force the demanded half of a candidate's
    /// result. The declaration environment and the call substitution compose
    /// into ONE map applied once; a body recipe is forced through the
    /// existing flow-return obligations; enumeration alone never gets here.
    ///
    /// The read works in `binding`'s epoch: the descriptor and call map are
    /// read there and the result is published there, whatever the store has
    /// made current since.
    pub fn read_signature_result(
        &self,
        binding: &EpochBinding,
        descriptor: SignatureDescriptorId,
        call_substitution: crate::signature_kernel::CallSubstitutionId,
        demand: ResultDemand,
        evaluation: ResultEvaluationContextId,
        context: SemanticContextId,
    ) -> QueryOutcome<AppliedResultId> {
        self.graph().record_signature_result_forced();
        let types = GraphTypes {
            dispatch: self,
            binding,
            semantic_identity: false,
        };
        match self.read_result_inner(
            &types,
            descriptor,
            call_substitution,
            demand,
            evaluation,
            context,
        ) {
            Ok(id) => QueryOutcome::Ready(Ready {
                value: id,
                evidence: CONTEXT_FREE_EVIDENCE,
            }),
            Err(error) => QueryOutcome::Incomplete(error.incomplete_reason()),
        }
    }

    fn read_result_inner(
        &self,
        types: &GraphTypes<'_, '_, C>,
        descriptor: SignatureDescriptorId,
        call_substitution: crate::signature_kernel::CallSubstitutionId,
        demand: ResultDemand,
        evaluation: ResultEvaluationContextId,
        context: SemanticContextId,
    ) -> Result<AppliedResultId, DiscoveryError> {
        let binding = types.binding;
        let view = binding.view();
        let desc = *view.descriptor(descriptor)?;
        let template = *view.template(desc.template)?;
        let recipe = *view.recipe(template.result_recipe)?;
        let return_type = if demand.reads_return() {
            let node = self.recipe_return(
                types,
                view,
                descriptor,
                call_substitution,
                evaluation,
                &recipe,
            )?;
            Some(binding.intern_type_token(node, None)?)
        } else {
            None
        };
        let effects = if demand.reads_effects() {
            match self.recipe_effects(
                types,
                view,
                descriptor,
                call_substitution,
                evaluation,
                &recipe,
            )? {
                Some(predicate) => Some(crate::signature_kernel::PredicateEffect {
                    subject: predicate.subject,
                    asserts: predicate.asserts,
                    ty: predicate
                        .ty
                        .map(|node| binding.intern_type_token(node, None))
                        .transpose()?,
                }),
                None => None,
            }
        } else {
            None
        };
        let record = AppliedResult {
            descriptor,
            substitution: call_substitution,
            recipe: template.result_recipe,
            evaluation,
            semantic_context: context,
            evidence: CONTEXT_FREE_EVIDENCE,
            return_type,
            effects,
        };
        let raw = binding.publish_result(record, None)?;
        Ok(AppliedResultId::from_raw(raw))
    }

    fn recipe_effects(
        &self,
        types: &GraphTypes<'_, '_, C>,
        view: &SemanticReadView,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
        evaluation: ResultEvaluationContextId,
        recipe: &SignatureResultRecipe,
    ) -> Result<Option<crate::semantic_query::SignaturePredicate>, DiscoveryError> {
        match recipe {
            SignatureResultRecipe::Declared {
                predicate_or_assertion: Some(effect),
                ..
            } => {
                // The target composes through the same declaration + call
                // maps as the return: `x is T` under `T := string` narrows
                // to `string`.
                let ty = match effect.ty {
                    Some(token) => {
                        let base = view.type_token_node(token)?;
                        let map = self.composed_map(types.binding, descriptor, call)?;
                        Some(self.substitute_canonical(base, &map))
                    }
                    None => None,
                };
                Ok(Some(crate::semantic_query::SignaturePredicate {
                    subject: effect.subject,
                    asserts: effect.asserts,
                    ty,
                }))
            }
            SignatureResultRecipe::Declared {
                predicate_or_assertion: None,
                ..
            } => Ok(None),
            // A body-derived result's effect is the predicate the checker
            // infers from the body: the same body obligation the return
            // read forces, under the same key. An unrecoverable body forces
            // nothing.
            SignatureResultRecipe::Body { .. } => {
                let key = self.body_flow_key(types.binding, descriptor, call, evaluation)?;
                self.graph().store().signature_store().note_body_forced();
                Ok(self.forced_body_result(key)?.inferred_predicate())
            }
            SignatureResultRecipe::UnionCommon { constituents, .. }
            | SignatureResultRecipe::UnionSynthesized { constituents, .. } => {
                let sequence = view.sequence(*constituents)?.clone();
                self.union_effects(types, view, &sequence, call, evaluation)
            }
            // A mixin construct signature is its base constructor's
            // signature with the mixed-in results: the base's own effect.
            SignatureResultRecipe::IntersectionConstruct {
                base_constructor,
                mixins,
            } => {
                let sequence = view.sequence(*mixins)?.clone();
                for edge in sequence.edges.iter() {
                    if types.binding.descriptor_source(edge.declaration)? == Some(*base_constructor)
                    {
                        let desc = view.descriptor(edge.residual)?;
                        let template = view.template(desc.template)?;
                        let recipe = *view.recipe(template.result_recipe)?;
                        return self.recipe_effects(
                            types,
                            view,
                            edge.residual,
                            call,
                            evaluation,
                            &recipe,
                        );
                    }
                }
                Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))
            }
        }
    }

    /// The effect of a UNION signature — the checker's
    /// `getUnionOrIntersectionTypePredicate` over the constituents, never
    /// an OR of their effects. Every constituent carries a type predicate
    /// (`x is T` / `this is T`) of the same kind about the same subject,
    /// and the union signature's predicate targets the union of theirs;
    /// a constituent without a predicate is admitted only when it returns
    /// exactly `false`. Any other constituent — a `boolean` or `true`
    /// return, an assertion, a predicate about a different subject —
    /// leaves the union signature with no predicate.
    fn union_effects(
        &self,
        types: &GraphTypes<'_, '_, C>,
        view: &SemanticReadView,
        sequence: &crate::signature_kernel::ConstituentSequence,
        call: crate::signature_kernel::CallSubstitutionId,
        evaluation: ResultEvaluationContextId,
    ) -> Result<Option<crate::semantic_query::SignaturePredicate>, DiscoveryError> {
        let mut last: Option<crate::semantic_query::SignaturePredicate> = None;
        let mut targets = Vec::with_capacity(sequence.edges.len());
        for edge in sequence.edges.iter() {
            let desc = view.descriptor(edge.residual)?;
            let template = view.template(desc.template)?;
            let recipe = *view.recipe(template.result_recipe)?;
            match self.recipe_effects(types, view, edge.residual, call, evaluation, &recipe)? {
                Some(predicate) => {
                    let Some(target) = predicate.ty.filter(|_| !predicate.asserts) else {
                        return Ok(None);
                    };
                    if last.is_some_and(|last| last.subject != predicate.subject) {
                        return Ok(None);
                    }
                    last = Some(predicate);
                    targets.push(target);
                }
                None => {
                    let returned =
                        self.recipe_return(types, view, edge.residual, call, evaluation, &recipe)?;
                    if self.graph().node_data(returned).as_deref()
                        != Some(&SemanticNodeData::Literal(
                            crate::semantic_query::LiteralValue::Boolean(false),
                        ))
                    {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(last.map(|last| crate::semantic_query::SignaturePredicate {
            ty: Some(self.intern_normalized_union_or_intersection(&targets, true)),
            ..last
        }))
    }

    /// The flow-return key of a body recipe's obligation under `call`: the
    /// served function position its source signature names, instantiated
    /// with the composed map's image of each declared binder (`unknown`
    /// where the map leaves one open).
    fn body_flow_key(
        &self,
        binding: &EpochBinding,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
        evaluation: ResultEvaluationContextId,
    ) -> Result<crate::semantic_query::FlowReturnKey, DiscoveryError> {
        let view = binding.view();
        let Some(source) = binding.descriptor_source(descriptor)? else {
            return Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput));
        };
        let node = view.type_token_node(source)?;
        let identity = {
            let graph = self.graph();
            let data = graph.node_data(node);
            match data.as_deref() {
                Some(SemanticNodeData::Signature { return_carrier, .. }) => return_carrier
                    .flow_identity()
                    .cloned()
                    .ok_or(DiscoveryError::Incomplete(
                        IncompleteReason::UnresolvedObligation,
                    ))?,
                Some(SemanticNodeData::DeferredCallable(callable)) => {
                    match callable
                        .parts(&ResolveOverloadSetConsumer::witness())
                        .return_carrier
                        .result_source()
                    {
                        crate::semantic_query::SignatureResultSource::Function(
                            verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                        ) => identity.clone(),
                        _ => {
                            return Err(DiscoveryError::Incomplete(
                                IncompleteReason::UnresolvedObligation,
                            ))
                        }
                    }
                }
                _ => {
                    return Err(DiscoveryError::Incomplete(
                        IncompleteReason::UnresolvedObligation,
                    ))
                }
            }
        };
        let map = self.composed_map(binding, descriptor, call)?;
        let unknown = self
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let desc = view.descriptor(descriptor)?;
        let env = view.environment(desc.declaration_environment)?;
        let mut params: Vec<SemanticNodeId> = env.bindings().iter().map(|(p, _)| *p).collect();
        params.sort_by_key(|p| {
            env.bindings()
                .iter()
                .find(|(param, _)| param == p)
                .and_then(|(_, token)| SignatureStore::binder_token_ordinal(*token))
                .unwrap_or(u32::MAX)
        });
        let args: Vec<SemanticNodeId> = params
            .iter()
            .map(|p| {
                map.bindings()
                    .iter()
                    .find_map(|(param, bound)| (param == p).then_some(*bound))
                    .unwrap_or(unknown)
            })
            .collect();
        Ok(self.flow_return_key_for_instantiation_with_evaluation(
            &identity,
            Arc::from(args.into_boxed_slice()),
            map,
            evaluation,
        ))
    }

    /// The completed flow-return value of a body obligation.
    fn forced_body_result(
        &self,
        key: crate::semantic_query::FlowReturnKey,
    ) -> Result<crate::semantic_query::FlowReturnResult, DiscoveryError> {
        let read = self.observe_flow_return_value(key.clone());
        let independent = match read.value() {
            crate::semantic_query::FlowReturnStep::Complete(result)
                if result.independent_member(&key).is_some() =>
            {
                Some(result.clone())
            }
            _ => None,
        };
        let step = if let Some(result) = independent {
            read.adopt_independent_member(self, &key)
                .map(|_| crate::semantic_query::FlowReturnStep::Complete(result))
        } else {
            read.adopt(self)
        };
        match step.unwrap_or_else(crate::semantic_query::FlowReturnStep::Aborted) {
            crate::semantic_query::FlowReturnStep::Complete(result) => Ok(result),
            crate::semantic_query::FlowReturnStep::NoValue(
                crate::semantic_query::FlowReturnFailure::Budget(_),
            ) => Err(DiscoveryError::Incomplete(IncompleteReason::Budget)),
            crate::semantic_query::FlowReturnStep::Hold(_)
            | crate::semantic_query::FlowReturnStep::Aborted(_)
            | crate::semantic_query::FlowReturnStep::NoValue(_) => Err(DiscoveryError::Incomplete(
                IncompleteReason::UnresolvedObligation,
            )),
        }
    }

    /// The single composed map for `descriptor` under `call`: declaration
    /// environment first, call substitution second, dropping any binding
    /// whose image is still an unresolved residual binder token.
    fn composed_map(
        &self,
        binding: &EpochBinding,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, DiscoveryError> {
        let view = binding.view();
        let desc = view.descriptor(descriptor)?;
        let env = view.environment(desc.declaration_environment)?;
        let call_map = binding.flatten_substitution(call)?;
        let composed = crate::signature_kernel::compose_canonical(env, &call_map);
        let kept: Vec<(SemanticNodeId, SemanticNodeId)> = composed
            .bindings()
            .iter()
            .copied()
            .filter(|(_, image)| !SignatureStore::is_binder_token(*image))
            .collect();
        Ok(CanonicalTypeSubstitution::new(self.graph().store(), kept))
    }

    fn recipe_return(
        &self,
        types: &GraphTypes<'_, '_, C>,
        view: &SemanticReadView,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
        evaluation: ResultEvaluationContextId,
        recipe: &SignatureResultRecipe,
    ) -> Result<SemanticNodeId, DiscoveryError> {
        match recipe {
            SignatureResultRecipe::Declared { return_type, .. } => {
                let base = view.type_token_node(*return_type)?;
                let map = self.composed_map(types.binding, descriptor, call)?;
                Ok(self.substitute_canonical(base, &map))
            }
            SignatureResultRecipe::Body { .. } => {
                self.graph().store().signature_store().note_body_forced();
                let key = self.body_flow_key(types.binding, descriptor, call, evaluation)?;
                Ok(self.forced_body_result(key)?.return_type())
            }
            SignatureResultRecipe::UnionCommon { constituents, .. }
            | SignatureResultRecipe::UnionSynthesized { constituents, .. }
            | SignatureResultRecipe::IntersectionConstruct {
                mixins: constituents,
                ..
            } => {
                let sequence = view.sequence(*constituents)?.clone();
                let mut returns = Vec::with_capacity(sequence.edges.len());
                for edge in sequence.edges.iter() {
                    let desc = view.descriptor(edge.residual)?;
                    let template = view.template(desc.template)?;
                    let recipe = *view.recipe(template.result_recipe)?;
                    returns.push(self.recipe_return(
                        types,
                        view,
                        edge.residual,
                        call,
                        evaluation,
                        &recipe,
                    )?);
                }
                let is_union =
                    !matches!(recipe, SignatureResultRecipe::IntersectionConstruct { .. });
                Ok(self.intern_normalized_union_or_intersection(&returns, is_union))
            }
        }
    }
}

/// The constituent sequence of a composite candidate; `None` for a leaf.
fn composite_sequence(
    view: &SemanticReadView,
    descriptor: SignatureDescriptorId,
) -> Result<Option<&crate::signature_kernel::ConstituentSequence>, DiscoveryError> {
    let desc = view.descriptor(descriptor)?;
    let template = view.template(desc.template)?;
    Ok(match view.recipe(template.result_recipe)? {
        SignatureResultRecipe::UnionCommon { constituents, .. }
        | SignatureResultRecipe::UnionSynthesized { constituents, .. }
        | SignatureResultRecipe::IntersectionConstruct {
            mixins: constituents,
            ..
        } => Some(view.sequence(*constituents)?),
        SignatureResultRecipe::Declared { .. } | SignatureResultRecipe::Body { .. } => None,
    })
}

/// The parameters and binders of one authored signature node.
type AuthoredSignatureParts = (
    Arc<[crate::semantic_query::FunctionParam]>,
    Arc<[crate::semantic_query::TypeParamDecl]>,
);

/// Void facts for a read that never asks for an arity minimum.
/// [`PositionalShape::type_at`] and [`PositionalShape::receiver`] do not
/// consult them; only the effective-minimum accessors do, and this shape is
/// never asked for one.
struct NoVoidFacts;

impl crate::signature_kernel::SlotTypeFacts for NoVoidFacts {
    fn accepts_void(&self, _ty: crate::signature_kernel::TypeToken) -> bool {
        false
    }
}

const NO_VOID_FACTS: NoVoidFacts = NoVoidFacts;

/// The type one shared candidate declares at one argument position.
#[derive(Debug, Clone)]
pub(crate) enum PositionalArgument {
    /// The candidate declares nothing at that position and has no rest run.
    Absent,
    Type {
        /// The declared type at the position.
        ty: SemanticNodeId,
        /// Its union arms without `null` / `undefined`, in union order; a
        /// non-union is its own single arm, and an all-nullish type is the
        /// empty list.
        non_nullish_arms: Vec<SemanticNodeId>,
        /// The position is optional and its type therefore includes
        /// `undefined` beside `ty` (an optional parameter under
        /// `strictNullChecks`).
        includes_undefined: bool,
    },
}

/// One candidate's positional slots as read under the pinned view, before
/// any semantics is dispatched over them.
pub struct RawPositional {
    pub receiver: Option<SemanticNodeId>,
    /// `None` when the candidate declares nothing at the position; else the
    /// declared type and whether its optionality adds `undefined`.
    pub argument: Option<(SemanticNodeId, bool)>,
}

/// One shared candidate's positional read.
#[derive(Debug, Clone)]
pub(super) struct PositionalRead {
    /// The candidate's authored `this` receiver, if it declares one. NEVER a
    /// positional slot — the positional model excludes it from every arity
    /// and every position — so a consumer that must judge receiver
    /// eligibility reads it here and asks the relation authority.
    pub receiver: Option<SemanticNodeId>,
    pub argument: PositionalArgument,
}

/// The positional read of every shared candidate of one subject: one read
/// per candidate in candidate order (empty is a complete negative), or the
/// reason the subject did not settle / a candidate's position carries no
/// single type.
pub(super) type SharedPositionalReads = Result<Vec<PositionalRead>, IncompleteReason>;

/// The parameter positions one signature comparison relates — see
/// [`ProjectSemanticDispatch::signature_comparison_plan`].
#[derive(Debug, Clone)]
pub struct SignatureComparisonPlan {
    /// The source demands more arguments than the target supplies.
    pub source_has_more_parameters: bool,
    /// Each signature's authored `this` receiver, when it declares one.
    pub source_receiver: Option<SemanticNodeId>,
    pub target_receiver: Option<SemanticNodeId>,
    /// `(source type, target type)` at each position both signatures
    /// declare a type at, in position order.
    pub positions: Vec<(SemanticNodeId, SemanticNodeId)>,
}

/// [`SignatureComparisonPlan`] as read under a pinned view, before the
/// positional types are materialized.
struct RawComparisonPlan {
    source_has_more_parameters: bool,
    source_receiver: Option<SemanticNodeId>,
    target_receiver: Option<SemanticNodeId>,
    positions: Vec<(RawPosition, RawPosition)>,
}

/// The type one signature declares at one compared position.
enum RawPosition {
    Absent,
    One(SemanticNodeId),
    /// An optional parameter whose type includes `undefined` under
    /// `strictNullChecks` (`getTypeOfParameter` adds it): its declared type
    /// and `undefined`.
    Optional(SemanticNodeId),
    /// A rest run with a tail: the union of the run element and the tail.
    Run(Vec<SemanticNodeId>),
    /// A still-generic rest indexed by the position (`T[index]`).
    GenericIndex {
        rest: SemanticNodeId,
        index: usize,
    },
    /// The rest of the parameter list from the position on.
    Remaining(Vec<RawRestElement>),
}

/// One element of the rest of a parameter list.
enum RawRestElement {
    /// A rest: its array (`array`, over the element `node`) or its
    /// still-generic type, named as the rest parameter is.
    Whole {
        node: SemanticNodeId,
        array: bool,
        label: Option<Arc<str>>,
    },
    /// One parameter, labelled with its name (the checker's
    /// `getRestTypeAtPosition` names each element after its parameter).
    Element {
        node: SemanticNodeId,
        optional: bool,
        label: Option<Arc<str>>,
    },
}

/// The ordered shared candidates of one subject as graph signature nodes.
pub enum SharedSignatureNodes {
    /// Every candidate, in candidate order. Empty is a complete negative.
    Nodes(Vec<SemanticNodeId>),
    /// The subject did not settle, or a candidate has no node form.
    Incomplete(IncompleteReason),
}

/// The candidates of one dispatched `SignaturesOfType` set, pinned in the
/// order they are read, while the node form of its composites is built:
/// each composite reads its applied result before it is a node.
pub(super) struct PinnedSignatureList {
    kind: GraphSignatureKind,
    slots: Vec<PinnedSignature>,
    /// The first slot not yet a node.
    next: usize,
    /// The composite at `next`, prepared for its result read.
    prepared: Option<PreparedCompositeSignature>,
    /// The epoch the unsettled composites' descriptors and the prepared
    /// call map live in: the list's own, until a rebind moves them.
    binding: EpochBinding,
}

enum PinnedSignature {
    Node(SemanticNodeId),
    Composite {
        descriptor: SignatureDescriptorId,
        constituents: Vec<SemanticNodeId>,
    },
}

/// A composite candidate's node form up to its result read.
struct PreparedCompositeSignature {
    descriptor: SignatureDescriptorId,
    call: crate::signature_kernel::CallSubstitutionId,
    params: Arc<[crate::semantic_query::FunctionParam]>,
    type_parameters: Arc<[crate::semantic_query::TypeParamDecl]>,
}

// A pinned list is owned state a suspended family carries to whichever
// thread resumes it, never shared by copy.
static_assertions::assert_impl_all!(PinnedSignatureList: Send);
static_assertions::assert_not_impl_any!(PinnedSignatureList: Clone, Copy);

impl PinnedSignatureList {
    /// The node of every candidate, in the order the list was pinned in.
    pub(super) fn into_nodes(self) -> Vec<SemanticNodeId> {
        self.slots
            .into_iter()
            .map(|slot| match slot {
                PinnedSignature::Node(node) => node,
                PinnedSignature::Composite { .. } => {
                    unreachable!("every composite settles before the list is read")
                }
            })
            .collect()
    }

    /// The epoch the list's unsettled composites are named in.
    pub(super) fn epoch(&self) -> crate::signature_kernel::GraphEpoch {
        self.binding.epoch()
    }

    /// Record the node form `node` of the prepared composite and move on.
    fn settle_next(&mut self, node: SemanticNodeId) {
        self.slots[self.next] = PinnedSignature::Node(node);
        self.next += 1;
    }
}

/// The parameter layout, receiver and flags `candidate` declares, read in
/// `view`.
fn signature_parts(
    view: &SemanticReadView,
    candidate: SignatureCandidate,
) -> Result<
    (
        &crate::signature_kernel::ParameterLayout,
        Option<crate::signature_kernel::ParameterSlot>,
        SignatureSemanticFlags,
    ),
    crate::signature_kernel::ReadError,
> {
    let descriptor = view.descriptor(candidate.signature)?;
    let template = view.template(descriptor.template)?;
    let shape = view.shape(template.input_shape)?;
    let layout = view.layout(shape.parameter_layout)?;
    let receiver = match shape.this_parameter {
        Some(id) => Some(*view.slot(id)?),
        None => None,
    };
    Ok((layout, receiver, shape.signature_semantic_flags))
}

/// The one candidate of a set that has exactly one, read in `view`.
fn only_candidate(
    view: &SemanticReadView,
    value: &crate::signature_kernel::SignatureSetValue,
) -> Result<SignatureCandidate, DiscoveryError> {
    match view.read_set(value.set)? {
        crate::signature_kernel::BorrowedSet::One { candidate, .. } => Ok(candidate),
        _ => unsettled(),
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Dispatch the subject's shared `SignaturesOfType` read. Anything but a
    /// signature set is the reason the subject did not settle.
    pub fn signature_set_value(
        &self,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
    ) -> Result<crate::signature_kernel::SignatureSetValue, IncompleteReason> {
        use crate::semantic_query::SemanticQueryKey;
        let read = self.execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
            subject,
            kind,
            context: SemanticContextId::production(),
        });
        self.signature_set_of_read(read)
    }

    /// The signature set a subject's `SignaturesOfType` read answered,
    /// wherever the read was made: anything but a signature set is the
    /// reason the subject did not settle.
    pub(super) fn signature_set_of_read(
        &self,
        read: crate::semantic_query::CacheRead<
            crate::semantic_query::QueryResult<crate::semantic_query::SemanticQueryValue>,
        >,
    ) -> Result<crate::signature_kernel::SignatureSetValue, IncompleteReason> {
        use crate::semantic_query::{QueryResult, SemanticQueryValue};
        if !read.fact_status.is_complete() {
            return Err(if self.connected_demand_tripped() {
                IncompleteReason::Budget
            } else {
                IncompleteReason::UnsettledInput
            });
        }
        match read.value {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => Ok(value),
            _ => Err(if self.connected_demand_tripped() {
                IncompleteReason::Budget
            } else {
                IncompleteReason::UnsettledInput
            }),
        }
    }

    /// The epoch a dispatched set's handles are read in: the one its own
    /// lease holds, current or retired. A value without a lease reads in the
    /// current epoch only when its handles belong to it.
    fn signature_value_binding(
        &self,
        value: &crate::signature_kernel::SignatureSetValue,
    ) -> Result<EpochBinding, IncompleteReason> {
        match &value.version.0 {
            Some(lease) => Ok(lease.bind()),
            None => {
                let binding = self.graph().store().signature_store().bind_current();
                if value
                    .set
                    .epoch()
                    .is_none_or(|epoch| epoch == binding.epoch())
                {
                    Ok(binding)
                } else {
                    Err(IncompleteReason::UnsettledInput)
                }
            }
        }
    }

    construction_ordinal_seam! {
        /// The subject's candidates of `kind`, read from `SignaturesOfType`, each
        /// as the graph signature node a node-based consumer reads: the authored
        /// node for a leaf, and for a composite the node form of its descriptor
        /// (the kernel's parameter layout and binders, with the return read
        /// through `ReadSignatureResult`). No signature is chosen, merged or
        /// reordered here.
        fn shared_signature_nodes(
            &self,
            subject: SemanticNodeId,
            kind: GraphSignatureKind,
        ) -> SharedSignatureNodes {
            let first = self.signature_set_value(subject, kind);
            self.shared_signature_nodes_from(kind, first)
        }
    }

    construction_ordinal_seam! {
        /// [`Self::shared_signature_nodes`] over an already-dispatched first
        /// read. The set is read in its own epoch, whichever epoch is current
        /// now: nothing is dispatched again.
        fn shared_signature_nodes_from(
            &self,
            kind: GraphSignatureKind,
            first: Result<crate::signature_kernel::SignatureSetValue, IncompleteReason>,
        ) -> SharedSignatureNodes {
            match first.and_then(|value| self.signature_nodes_of(kind, &value)) {
                Ok(nodes) => SharedSignatureNodes::Nodes(nodes),
                Err(reason) => SharedSignatureNodes::Incomplete(reason),
            }
        }
    }

    /// The node form of every candidate of one dispatched set, in candidate
    /// order. Each composite's result is read and published in the set's
    /// own epoch, so no query key is named and nothing is rebound.
    fn signature_nodes_of(
        &self,
        kind: GraphSignatureKind,
        value: &crate::signature_kernel::SignatureSetValue,
    ) -> Result<Vec<SemanticNodeId>, IncompleteReason> {
        let mut pinned = self.pin_signature_list(kind, value, false)?;
        while let Some(key) = self.prepare_next_composite(&mut pinned)? {
            let applied = match self.read_signature_result(
                &pinned.binding,
                key.descriptor,
                key.call_substitution,
                key.projection,
                key.evaluation,
                key.semantic_context,
            ) {
                QueryOutcome::Ready(Ready { value, .. }) => value,
                QueryOutcome::Incomplete(reason) => return Err(reason),
            };
            self.settle_composite_in_binding(&mut pinned, applied)?;
        }
        Ok(pinned.into_nodes())
    }

    /// The candidates of one dispatched set, pinned in candidate order — or,
    /// with `resolution`, in the kernel's resolution order: each authored
    /// candidate as its node, each composite as the result read its node
    /// form waits on ([`Self::prepare_next_composite`]). The set is read in
    /// its own epoch, which the list keeps.
    pub(super) fn pin_signature_list(
        &self,
        kind: GraphSignatureKind,
        value: &crate::signature_kernel::SignatureSetValue,
        resolution: bool,
    ) -> Result<PinnedSignatureList, IncompleteReason> {
        #[cfg(any(test, feature = "test-support"))]
        signatures_of_type_build_point(SignaturesOfTypeBuildPoint::Pinning);
        let unsettled = IncompleteReason::UnsettledInput;
        let binding = self.signature_value_binding(value)?;
        let (candidates, order): (Vec<SignatureCandidate>, Vec<usize>) = {
            let view = binding.view();
            let candidates = match view.read_set(value.set) {
                Ok(crate::signature_kernel::BorrowedSet::Empty) => Vec::new(),
                Ok(crate::signature_kernel::BorrowedSet::One { candidate, .. }) => vec![candidate],
                Ok(crate::signature_kernel::BorrowedSet::Many(list)) => list.to_vec(),
                Err(_) => return Err(unsettled),
            };
            let order = if resolution {
                crate::signature_kernel::resolution_order(view, &candidates)
                    .map_err(DiscoveryError::incomplete_reason)?
            } else {
                (0..candidates.len()).collect()
            };
            (candidates, order)
        };
        if candidates.len() != value.nodes.len() {
            return Err(unsettled);
        }
        let slots = order
            .into_iter()
            .map(|index| match value.nodes[index].authored {
                Some(node) => PinnedSignature::Node(node),
                None => PinnedSignature::Composite {
                    descriptor: candidates[index].signature,
                    constituents: value.nodes[index].constituents.to_vec(),
                },
            })
            .collect();
        Ok(PinnedSignatureList {
            kind,
            slots,
            next: 0,
            prepared: None,
            binding,
        })
    }

    /// Prepare the node form of `list`'s next composite candidate in the
    /// list's epoch, settling each authored candidate before it: the result
    /// read it waits on, or `None` once every candidate is a node.
    pub(super) fn prepare_next_composite(
        &self,
        list: &mut PinnedSignatureList,
    ) -> Result<Option<crate::signature_kernel::ReadSignatureResultKey>, IncompleteReason> {
        while let Some(slot) = list.slots.get(list.next) {
            let PinnedSignature::Composite {
                descriptor,
                constituents,
            } = slot
            else {
                list.next += 1;
                continue;
            };
            let prepared = self
                .prepare_composite_signature(&list.binding, *descriptor, constituents)
                .map_err(DiscoveryError::incomplete_reason)?;
            let key = crate::signature_kernel::ReadSignatureResultKey {
                descriptor: prepared.descriptor,
                call_substitution: prepared.call,
                projection: ResultDemand::Both,
                evaluation: CONTEXT_FREE_EVALUATION,
                semantic_context: SemanticContextId::production(),
            };
            list.prepared = Some(prepared);
            return Ok(Some(key));
        }
        Ok(None)
    }

    /// The result read `list`'s next composite candidate waits on, named in
    /// the current epoch: a list whose epoch a replacement retired has its
    /// unsettled composites copied into the current one first, so the read
    /// is never named from retired handles. `None` once every candidate is
    /// a node.
    pub(super) fn name_next_composite_result(
        &self,
        list: &mut PinnedSignatureList,
    ) -> Result<Option<crate::signature_kernel::ReadSignatureResultKey>, IncompleteReason> {
        while matches!(list.slots.get(list.next), Some(PinnedSignature::Node(_))) {
            list.next += 1;
        }
        if list.next == list.slots.len() {
            return Ok(None);
        }
        if list.binding.epoch() != self.graph().store().signature_store().epoch() {
            self.rebind_pinned_composites(list)?;
        }
        self.prepare_next_composite(list)
    }

    /// Copy `list`'s unsettled composite descriptors from the list's retired
    /// epoch into the current one, in one pass, and bind the list there. The
    /// settled candidates are graph nodes and need no copy; a prepared call
    /// map of the retired epoch is dropped.
    #[cold]
    #[inline(never)]
    fn rebind_pinned_composites(
        &self,
        list: &mut PinnedSignatureList,
    ) -> Result<(), IncompleteReason> {
        let into = self.graph().store().signature_store().bind_current();
        let mut rebind = EpochRebind::new(&list.binding.lease(), &into);
        for slot in &mut list.slots[list.next..] {
            if let PinnedSignature::Composite { descriptor, .. } = slot {
                *descriptor = rebind
                    .descriptor(*descriptor)
                    .map_err(|error| DiscoveryError::Store(error).incomplete_reason())?;
            }
        }
        let copied = rebind.descriptors_copied();
        drop(rebind);
        list.binding = into;
        list.prepared = None;
        self.graph().record_signature_rebind(copied);
        Ok(())
    }

    /// Settle `list`'s prepared composite candidate with the result its
    /// read delivered. The applied record is read where the result was
    /// published, through the result's own lease, whichever epoch is
    /// current now: a result forced before its epoch retired is consumed,
    /// never forced again.
    pub(super) fn settle_composite_signature(
        &self,
        list: &mut PinnedSignatureList,
        result: &crate::signature_kernel::SignatureResultValue,
    ) -> Result<(), IncompleteReason> {
        let prepared = list
            .prepared
            .take()
            .expect("a composite settles with the result its preparation read");
        let node = match &result.version.0 {
            Some(lease) => self.finish_composite_signature(
                &lease.read_view(),
                list.kind,
                prepared,
                result.result,
            ),
            None if result.result.epoch() == list.binding.epoch() => self
                .finish_composite_signature(
                    list.binding.view(),
                    list.kind,
                    prepared,
                    result.result,
                ),
            None => return Err(IncompleteReason::UnsettledInput),
        }
        .map_err(DiscoveryError::incomplete_reason)?;
        list.settle_next(node);
        Ok(())
    }

    /// Settle `list`'s prepared composite candidate with a result read and
    /// published in the list's own epoch.
    fn settle_composite_in_binding(
        &self,
        list: &mut PinnedSignatureList,
        applied: AppliedResultId,
    ) -> Result<(), IncompleteReason> {
        let prepared = list
            .prepared
            .take()
            .expect("a composite settles with the result its preparation read");
        let node = self
            .finish_composite_signature(list.binding.view(), list.kind, prepared, applied)
            .map_err(DiscoveryError::incomplete_reason)?;
        list.settle_next(node);
        Ok(())
    }

    /// The type at positional argument `position` of every shared candidate
    /// of `subject`, in candidate order, read through the ONE positional
    /// model ([`crate::signature_kernel::PositionalShape`]).
    ///
    /// This is the shape accessor every consumer that needs "the type an
    /// argument lands on" reads: the receiver (`this`) is never a positional
    /// slot and is reported separately, a leading array rest contributes its
    /// element (`...cbs: F[]` at 0 is `F`), a fixed tuple rest is already
    /// flattened into ordinary positions (`...args: [F, G]` at 0 is `F`), and
    /// a position past the last declared parameter of a rest-less signature
    /// is [`PositionalArgument::Absent`]. A still-generic rest and a rest run
    /// with a required tail are not a single position: both are an
    /// incomplete read rather than an invented type.
    ///
    /// An empty candidate list is a complete negative — `subject` carries no
    /// signature of `kind`, which is also the answer for a provably
    /// non-callable value (`any`, `never`, a primitive with no augmented
    /// apparent call signature).
    pub(super) fn shared_positional_reads(
        &self,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        position: usize,
    ) -> SharedPositionalReads {
        let first = self.signature_set_value(subject, kind)?;
        let raw = self.raw_positional_reads(&first, position)?;
        Ok(raw
            .into_iter()
            .map(|raw| PositionalRead {
                receiver: raw.receiver,
                argument: match raw.argument {
                    None => PositionalArgument::Absent,
                    Some((ty, includes_undefined)) => PositionalArgument::Type {
                        ty,
                        non_nullish_arms: self.non_nullish_arms(ty),
                        includes_undefined,
                    },
                },
            })
            .collect())
    }

    construction_ordinal_seam! {
        /// Every candidate's positional slots at `position`, read under ONE
        /// view of the set's own epoch; no semantics is dispatched while it is
        /// held.
        fn raw_positional_reads(
            &self,
            value: &crate::signature_kernel::SignatureSetValue,
            position: usize,
        ) -> Result<Vec<RawPositional>, IncompleteReason> {
            use crate::signature_kernel::{PositionalShape, TypeAt};

            let unsettled = IncompleteReason::UnsettledInput;
            let binding = self.signature_value_binding(value)?;
            let raw: Result<Vec<RawPositional>, ()> = {
                let view = binding.view();
                let candidates: Vec<SignatureCandidate> = match view.read_set(value.set) {
                    Ok(crate::signature_kernel::BorrowedSet::Empty) => Vec::new(),
                    Ok(crate::signature_kernel::BorrowedSet::One { candidate, .. }) => vec![candidate],
                    Ok(crate::signature_kernel::BorrowedSet::Many(list)) => list.to_vec(),
                    Err(_) => return Err(unsettled),
                };
                let mut out = Vec::with_capacity(candidates.len());
                let mut failed = false;
                for candidate in &candidates {
                    let Ok(descriptor) = view.descriptor(candidate.signature) else {
                        failed = true;
                        break;
                    };
                    let Ok(template) = view.template(descriptor.template) else {
                        failed = true;
                        break;
                    };
                    let Ok(shape) = view.shape(template.input_shape) else {
                        failed = true;
                        break;
                    };
                    let Ok(layout) = view.layout(shape.parameter_layout) else {
                        failed = true;
                        break;
                    };
                    let receiver_slot = match shape.this_parameter {
                        Some(id) => match view.slot(id) {
                            Ok(slot) => Some(*slot),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        },
                        None => None,
                    };
                    let positional = PositionalShape::new(
                        layout,
                        receiver_slot,
                        shape.signature_semantic_flags,
                        &NO_VOID_FACTS,
                    );
                    let argument = match positional.type_at(position) {
                        TypeAt::Absent => None,
                        TypeAt::One(slot) => match view.type_token_node(slot.ty) {
                            Ok(node) => Some((node, slot.optionality.includes_undefined)),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        },
                        // A rest run with a required tail, and a still-generic
                        // rest, are not a single positional type.
                        TypeAt::Run { .. } | TypeAt::GenericRest { .. } => {
                            failed = true;
                            break;
                        }
                    };
                    let receiver = match positional.receiver() {
                        Some(slot) => match view.type_token_node(slot.ty) {
                            Ok(node) => Some(node),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        },
                        None => None,
                    };
                    out.push(RawPositional { receiver, argument });
                }
                if failed {
                    Err(())
                } else {
                    Ok(out)
                }
            };
            raw.map_err(|()| unsettled)
        }
    }

    /// The source's one candidate in `binding`'s epoch: read in the
    /// source's own epoch and, when that is another one, copied into
    /// `binding`'s, so the comparison names the handles of one epoch.
    #[inline(never)]
    fn single_candidate_in(
        &self,
        source: &crate::signature_kernel::SignatureSetValue,
        binding: &EpochBinding,
    ) -> Result<SignatureCandidate, DiscoveryError> {
        let own = self
            .signature_value_binding(source)
            .map_err(DiscoveryError::Incomplete)?;
        let candidate = only_candidate(own.view(), source)?;
        if own.epoch() == binding.epoch() {
            return Ok(candidate);
        }
        let mut rebind = EpochRebind::new(&own.lease(), binding);
        let copy = rebind.candidate(candidate)?;
        self.graph()
            .record_signature_rebind(rebind.descriptors_copied());
        Ok(copy)
    }

    construction_ordinal_seam! {
        /// Exact signature identity uses the kernel's positional, binder and
        /// result rules, with component types compared by the relation
        /// authority. Each subject's set is dispatched once; the comparison
        /// works in the target's epoch, the source's candidate copied into it
        /// when it was read in another.
        fn signature_types_identical(
            &self,
            source: SemanticNodeId,
            target: SemanticNodeId,
            kind: GraphSignatureKind,
        ) -> Option<bool> {
            let source = self.signature_set_value(source, kind).ok()?;
            let target = self.signature_set_value(target, kind).ok()?;
            let binding = self.signature_value_binding(&target).ok()?;
            let compared = (|| {
                let target = only_candidate(binding.view(), &target)?;
                let source = self.single_candidate_in(&source, &binding)?;
                crate::signature_kernel::signatures_identical(
                    &binding,
                    &GraphTypes {
                        dispatch: self,
                        binding: &binding,
                        semantic_identity: true,
                    },
                    source,
                    target,
                    crate::signature_kernel::MatchOptions::EXACT,
                )
            })();
            compared.ok()
        }
    }

    construction_ordinal_seam! {
        /// The parameter positions one signature comparison relates, read
        /// through the ONE positional model for both signatures — the checker's
        /// `compareSignaturesRelated` over `getParameterCount`,
        /// `tryGetTypeAtPosition` and `getRestOrAnyTypeAtPosition`:
        ///
        /// - the arity half is [`crate::signature_kernel::PositionalShape::source_has_more_parameters`]:
        ///   a target with a rest run accepts every source, any other target
        ///   rejects a source whose minimum exceeds its parameter count — or,
        ///   under `strict_arity` (the checker's `StrictArity` mode, which its
        ///   strict subtype relation uses), a source with a rest parameter or
        ///   with more parameters than the target takes;
        /// - without a still-generic rest on either side, every position up to
        ///   the LARGER parameter count is related where both signatures declare
        ///   a type there — a rest run supplies its element at every position
        ///   past the fixed ones (`(...args: any[])` is `any` at 0, 1, …);
        /// - with a still-generic rest, positions run to the SMALLER count and
        ///   the last one relates the rest of each parameter list from there as
        ///   one tuple (`any` when that is an array of `any`).
        /// - an optional parameter's type includes `undefined` where its slot
        ///   says so (`strictNullChecks`, the checker's `getTypeOfParameter`).
        ///
        /// Each subject's set is dispatched once and read in its own epoch.
        ///
        /// Measured on 7.0.2: `((a: string) => void) extends ((...args: any[]) =>
        /// void)` and the `new` form are true, `((a: string, b: number) => void)
        /// extends ((...args: string[]) => void)` is false, `((...a: string[]) =>
        /// void) extends ((x: string, y: string) => void)` is true.
        fn signature_comparison_plan(
            &self,
            source: SemanticNodeId,
            target: SemanticNodeId,
            kind: GraphSignatureKind,
            strict_arity: bool,
        ) -> Result<SignatureComparisonPlan, IncompleteReason> {
            let source_set = self.signature_set_value(source, kind)?;
            let target_set = self.signature_set_value(target, kind)?;
            let read = self.raw_comparison_plan(&source_set, &target_set, strict_arity)?;
            let graph = self.graph();
            let materialize = |position: RawPosition| -> Option<SemanticNodeId> {
                match position {
                    RawPosition::Absent => None,
                    RawPosition::One(node) => Some(node),
                    RawPosition::Optional(node) => {
                        let undefined =
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                        Some(self.intern_normalized_union_or_intersection(&[node, undefined], true))
                    }
                    RawPosition::Run(members) => {
                        Some(self.intern_normalized_union_or_intersection(&members, true))
                    }
                    RawPosition::GenericIndex { rest, index } => {
                        let index = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
                            index as f64,
                        )));
                        Some(graph.intern_node(SemanticNodeData::IndexedAccess {
                            object: rest,
                            index: crate::semantic_query::IndexKey::Computed(index),
                            origin: crate::semantic_query::IndexedAccessOrigin::Composed,
                        }))
                    }
                    RawPosition::Remaining(elements) => {
                        let any_array = |node: SemanticNodeId| {
                            matches!(
                                graph.node_data(node).as_deref(),
                                Some(SemanticNodeData::Array { element, .. })
                                    if matches!(
                                        graph.node_data(*element).as_deref(),
                                        Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
                                    )
                            )
                        };
                        let rest_or_any = |node: SemanticNodeId| {
                            if any_array(node) {
                                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any))
                            } else {
                                node
                            }
                        };
                        let whole = |node: SemanticNodeId, array: bool| {
                            if array {
                                graph.intern_node(SemanticNodeData::Array {
                                    element: node,
                                    readonly: false,
                                })
                            } else {
                                node
                            }
                        };
                        Some(match elements.as_slice() {
                            [RawRestElement::Whole { node, array, .. }] => {
                                rest_or_any(whole(*node, *array))
                            }
                            _ => rest_or_any(
                                graph.intern_node(SemanticNodeData::Tuple {
                                    elements: elements
                                        .iter()
                                        .map(|element| match element {
                                            RawRestElement::Whole { node, array, label } => {
                                                crate::semantic_query::TupleElement {
                                                    label: label.as_deref().map(OwnedStr::from),
                                                    value: whole(*node, *array),
                                                    optional: false,
                                                    rest: true,
                                                }
                                            }
                                            RawRestElement::Element {
                                                node,
                                                optional,
                                                label,
                                            } => crate::semantic_query::TupleElement {
                                                label: label.as_deref().map(OwnedStr::from),
                                                value: *node,
                                                optional: *optional,
                                                rest: false,
                                            },
                                        })
                                        .collect(),
                                    readonly: false,
                                }),
                            ),
                        })
                    }
                }
            };
            let positions = read
                .positions
                .into_iter()
                .filter_map(|(source, target)| Some((materialize(source)?, materialize(target)?)))
                .collect();
            Ok(SignatureComparisonPlan {
                source_has_more_parameters: read.source_has_more_parameters,
                source_receiver: read.source_receiver,
                target_receiver: read.target_receiver,
                positions,
            })
        }
    }

    /// Both signatures' positional reads, each under a view of its own set's
    /// epoch; no semantics is dispatched while they are held.
    fn raw_comparison_plan(
        &self,
        source: &crate::signature_kernel::SignatureSetValue,
        target: &crate::signature_kernel::SignatureSetValue,
        strict_arity: bool,
    ) -> Result<RawComparisonPlan, IncompleteReason> {
        use crate::signature_kernel::{
            PositionalMode, PositionalShape, ProjectedKind, ProjectedTuple, RestKind, TypeAt,
        };

        let unsettled = IncompleteReason::UnsettledInput;
        let source_binding = self.signature_value_binding(source)?;
        let target_binding = self.signature_value_binding(target)?;
        let (source_view, target_view) = (source_binding.view(), target_binding.view());
        let source_facts = GraphTypes {
            dispatch: self,
            binding: &source_binding,
            semantic_identity: false,
        };
        let target_facts = GraphTypes {
            dispatch: self,
            binding: &target_binding,
            semantic_identity: false,
        };
        let (source_layout, source_this, source_flags) = signature_parts(
            source_view,
            only_candidate(source_view, source).map_err(|_| unsettled)?,
        )
        .map_err(|_| unsettled)?;
        let (target_layout, target_this, target_flags) = signature_parts(
            target_view,
            only_candidate(target_view, target).map_err(|_| unsettled)?,
        )
        .map_err(|_| unsettled)?;
        let source_shape =
            PositionalShape::new(source_layout, source_this, source_flags, &source_facts);
        let target_shape =
            PositionalShape::new(target_layout, target_this, target_flags, &target_facts);
        let node = |view: &SemanticReadView, token: TypeToken| {
            view.type_token_node(token).map_err(|_| unsettled)
        };
        // A rest of type `any` is `any` at every position, never a
        // still-generic rest (`getNonArrayRestType` excludes it).
        let generic_rest = |view: &SemanticReadView,
                            shape: &PositionalShape<'_>|
         -> Result<bool, IncompleteReason> {
            match shape.rest() {
                Some(rest) if rest.kind == RestKind::GenericTuple => {
                    let rest_node = node(view, rest.slot.ty)?;
                    Ok(!matches!(
                        self.graph().node_data(rest_node).as_deref(),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
                    ))
                }
                _ => Ok(false),
            }
        };
        let count = |shape: &PositionalShape<'_>| {
            shape.parameter_count() + shape.rest().map_or(0, |rest| rest.tail.len())
        };
        let source_generic = generic_rest(source_view, &source_shape)?;
        let target_generic = generic_rest(target_view, &target_shape)?;
        let (source_count, target_count) = (count(&source_shape), count(&target_shape));
        let generic = source_generic || target_generic;
        let compared = if generic {
            source_count.min(target_count)
        } else {
            source_count.max(target_count)
        };
        let rest_index = generic.then(|| compared.checked_sub(1)).flatten();
        let at = |view: &SemanticReadView,
                  shape: &PositionalShape<'_>,
                  pos: usize|
         -> Result<RawPosition, IncompleteReason> {
            Ok(match shape.type_at(pos) {
                TypeAt::Absent => RawPosition::Absent,
                TypeAt::One(slot) if slot.optionality.includes_undefined => {
                    RawPosition::Optional(node(view, slot.ty)?)
                }
                TypeAt::One(slot) => RawPosition::One(node(view, slot.ty)?),
                TypeAt::Run { element, tail } => {
                    let mut members = vec![node(view, element)?];
                    for slot in tail {
                        members.push(node(view, slot.ty)?);
                    }
                    RawPosition::Run(members)
                }
                TypeAt::GenericRest { rest, index } => {
                    let rest = node(view, rest)?;
                    if matches!(
                        self.graph().node_data(rest).as_deref(),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
                    ) {
                        RawPosition::One(rest)
                    } else {
                        RawPosition::GenericIndex { rest, index }
                    }
                }
            })
        };
        // `getRestTypeAtPosition`: the rest of the parameter list from
        // `pos` on, as the rest type itself at its own position, an array of
        // its element past it, else a tuple of the remaining positions.
        let remaining = |view: &SemanticReadView,
                         shape: &PositionalShape<'_>,
                         pos: usize|
         -> Result<RawPosition, IncompleteReason> {
            let label = |name: Option<crate::signature_kernel::SpellingId>| -> Result<Option<Arc<str>>, IncompleteReason> {
                name.map(|id| view.spelling(id).map(Arc::from).map_err(|_| unsettled))
                    .transpose()
            };
            let rest_node = |rest: &crate::signature_kernel::RestSlot| -> Result<RawRestElement, IncompleteReason> {
                Ok(RawRestElement::Whole {
                    node: node(view, rest.slot.ty)?,
                    array: rest.kind == RestKind::Array,
                    label: label(rest.slot.name)?,
                })
            };
            Ok(RawPosition::Remaining(
                match shape.project_tuple(pos, PositionalMode::Comparison) {
                    ProjectedTuple::Rest { exact: true, .. } => {
                        let rest = shape.rest().ok_or(unsettled)?;
                        vec![rest_node(rest)?]
                    }
                    ProjectedTuple::Rest { ty, exact: false } => {
                        let rest = shape.rest().ok_or(unsettled)?;
                        let element = match rest.kind {
                            RestKind::Array => node(view, ty)?,
                            RestKind::GenericTuple => return Ok(RawPosition::Absent),
                        };
                        vec![RawRestElement::Whole {
                            node: element,
                            array: true,
                            label: label(rest.slot.name)?,
                        }]
                    }
                    ProjectedTuple::Elements(elements) => {
                        let mut out = Vec::with_capacity(elements.len());
                        for element in elements {
                            out.push(match element.kind {
                                ProjectedKind::Variadic => {
                                    let rest = shape.rest().ok_or(unsettled)?;
                                    rest_node(rest)?
                                }
                                kind => RawRestElement::Element {
                                    node: node(view, element.ty)?,
                                    optional: kind == ProjectedKind::Optional,
                                    label: label(element.name)?,
                                },
                            });
                        }
                        out
                    }
                },
            ))
        };
        let mut positions = Vec::with_capacity(compared);
        for pos in 0..compared {
            if Some(pos) == rest_index {
                positions.push((
                    remaining(source_view, &source_shape, pos)?,
                    remaining(target_view, &target_shape, pos)?,
                ));
            } else {
                positions.push((
                    at(source_view, &source_shape, pos)?,
                    at(target_view, &target_shape, pos)?,
                ));
            }
        }
        Ok(RawComparisonPlan {
            source_has_more_parameters: PositionalShape::source_has_more_parameters(
                &source_shape,
                &target_shape,
                strict_arity,
                PositionalMode::Comparison,
            ),
            source_receiver: source_this
                .map(|slot| node(source_view, slot.ty))
                .transpose()?,
            target_receiver: target_this
                .map(|slot| node(target_view, slot.ty))
                .transpose()?,
            positions,
        })
    }

    /// `node`'s union arms without `null` / `undefined` (the checker's
    /// `NEUndefinedOrNull` facts); a non-union is its own single arm. The
    /// nullish split belongs to the positional read because every consumer
    /// of an argument position that may be omitted needs the same one.
    fn non_nullish_arms(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        let resolved = self
            .evaluate_deferred_semantic_node_with_context(
                node,
                ProjectionReductionContext::structural_transit(),
            )
            .into_active_query_build_node(self);
        let is_nullish = |arm: SemanticNodeId| {
            matches!(
                self.graph().node_data(arm).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::Null | PrimitiveKind::Undefined
                ))
            )
        };
        let members: Vec<SemanticNodeId> = match self.graph().node_data(resolved).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            _ => vec![resolved],
        };
        members
            .into_iter()
            .filter(|arm| !is_nullish(*arm))
            .collect()
    }

    construction_ordinal_seam! {
        /// Both buckets of the subject's shared list as signature nodes, call
        /// then construct; the reason either list did not settle otherwise.
        fn shared_signature_buckets(
            &self,
            subject: SemanticNodeId,
        ) -> Result<(Vec<SemanticNodeId>, Vec<SemanticNodeId>), IncompleteReason> {
            let bucket = |kind| match self.shared_signature_nodes(subject, kind) {
                SharedSignatureNodes::Nodes(nodes) => Ok(nodes),
                SharedSignatureNodes::Incomplete(reason) => Err(reason),
            };
            Ok((
                bucket(GraphSignatureKind::Call)?,
                bucket(GraphSignatureKind::Construct)?,
            ))
        }
    }

    /// The parameters and binders an authored signature node carries.
    fn authored_signature_parts(&self, node: SemanticNodeId) -> Option<AuthoredSignatureParts> {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                type_parameters,
                ..
            }) => Some((
                Arc::clone(params.as_arc()),
                Arc::clone(type_parameters.as_arc()),
            )),
            Some(SemanticNodeData::DeferredCallable(callable)) => {
                let parts = callable.parts(&ResolveOverloadSetConsumer::witness());
                Some((Arc::clone(parts.params), Arc::clone(parts.type_parameters)))
            }
            _ => None,
        }
    }

    /// What the node form of one composite descriptor computes before its
    /// result read: its parameter list and binders, in one constituent's
    /// declaration environment, and the call map naming each residual binder
    /// by its own declared parameter. `constituent_nodes` are the authored
    /// nodes of its constituents, in sequence order.
    fn prepare_composite_signature(
        &self,
        binding: &EpochBinding,
        descriptor: SignatureDescriptorId,
        constituent_nodes: &[SemanticNodeId],
    ) -> Result<PreparedCompositeSignature, DiscoveryError> {
        let view = binding.view();
        let desc = *view.descriptor(descriptor)?;
        let template = *view.template(desc.template)?;
        let shape = *view.shape(template.input_shape)?;
        let Some(sequence) = composite_sequence(view, descriptor)? else {
            return unsupported();
        };
        if sequence.edges.len() != constituent_nodes.len() || constituent_nodes.is_empty() {
            return unsettled();
        }
        // The composite keeps one constituent's declaration environment: that
        // constituent owns the binders, and one with the composite's own
        // input shape owns the parameter list.
        let mut binder_owner = None;
        let mut shape_owner = None;
        for (edge, node) in sequence.edges.iter().zip(constituent_nodes) {
            let constituent = view.descriptor(edge.declaration)?;
            if constituent.declaration_environment != desc.declaration_environment {
                continue;
            }
            binder_owner.get_or_insert(*node);
            if view.template(constituent.template)?.input_shape == template.input_shape {
                shape_owner.get_or_insert(*node);
            }
        }
        let binder_count = view.space(desc.residual_binders)?.binders.len();
        let type_parameters: Arc<[crate::semantic_query::TypeParamDecl]> = if binder_count == 0 {
            Arc::from(Vec::new().into_boxed_slice())
        } else {
            match binder_owner.and_then(|node| self.authored_signature_parts(node)) {
                Some((_, type_parameters)) if type_parameters.len() == binder_count => {
                    type_parameters
                }
                _ => return unsupported(),
            }
        };
        let params: Arc<[crate::semantic_query::FunctionParam]> =
            match shape_owner.and_then(|node| self.authored_signature_parts(node)) {
                Some((params, _)) => params,
                None => self.layout_params(view, &shape)?,
            };
        // The call map that names each residual binder by its own declared
        // parameter: the composite return reads in the binder owner's terms.
        let environment = view.environment(desc.declaration_environment)?;
        let naming: Vec<(SemanticNodeId, SemanticNodeId)> = environment
            .bindings()
            .iter()
            .filter(|(_, token)| SignatureStore::is_binder_token(*token))
            .map(|(param, token)| (*token, *param))
            .collect();
        let call = binding.intern_substitution(
            crate::signature_kernel::CallSubstitution::map(
                desc.residual_binders,
                CanonicalTypeSubstitution::new(self.graph().store(), naming),
            ),
            None,
        )?;
        Ok(PreparedCompositeSignature {
            descriptor,
            call,
            params,
            type_parameters,
        })
    }

    /// The graph node form of a prepared composite descriptor, given the
    /// result its call map applied, read in `view`: the epoch the result
    /// was published in.
    fn finish_composite_signature(
        &self,
        view: &SemanticReadView,
        kind: GraphSignatureKind,
        prepared: PreparedCompositeSignature,
        applied: AppliedResultId,
    ) -> Result<SemanticNodeId, DiscoveryError> {
        let PreparedCompositeSignature {
            params,
            type_parameters,
            ..
        } = prepared;
        let applied = view.applied_result(applied)?;
        let token = applied
            .return_type
            .ok_or(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))?;
        let predicate = match applied.effects {
            Some(effect) => Some(crate::semantic_query::SignaturePredicate {
                subject: effect.subject,
                asserts: effect.asserts,
                ty: effect
                    .ty
                    .map(|token| view.type_token_node(token))
                    .transpose()?,
            }),
            None => None,
        };
        let return_type = view.type_token_node(token)?;
        Ok(self.graph().intern_node(SemanticNodeData::Signature {
            kind,
            params: crate::semantic_query::PayloadArray::from(params),
            return_type,
            type_parameters: crate::semantic_query::PayloadArray::from(type_parameters),
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(return_type),
            signature_span: None,
            return_type_span: None,
            // The composite's effect: the union rule over its constituents,
            // or a mixin construct's base.
            predicate,
            is_abstract: false,
        }))
    }

    /// A synthesized parameter layout as graph parameters.
    fn layout_params(
        &self,
        view: &SemanticReadView,
        shape: &crate::signature_kernel::SignatureInputShape,
    ) -> Result<Arc<[crate::semantic_query::FunctionParam]>, DiscoveryError> {
        let layout = view.layout(shape.parameter_layout)?;
        let named = |slot: &crate::signature_kernel::ParameterSlot,
                     fallback: Option<&str>|
         -> Result<Option<Arc<str>>, DiscoveryError> {
            Ok(match slot.name {
                Some(id) => Some(Arc::from(view.spelling(id)?)),
                None => fallback.map(Arc::from),
            })
        };
        let mut params = Vec::with_capacity(layout.parameters.len() + 2);
        if let Some(receiver) = shape.this_parameter {
            let slot = *view.slot(receiver)?;
            params.push(crate::semantic_query::FunctionParam::synthetic(
                Some(Arc::from("this")),
                view.type_token_node(slot.ty)?,
                false,
                false,
            ));
        }
        for slot in layout.parameters.iter() {
            params.push(crate::semantic_query::FunctionParam::synthetic(
                named(slot, None)?,
                view.type_token_node(slot.ty)?,
                slot.optionality.declared_optional,
                false,
            ));
        }
        if let Some(rest) = &layout.rest {
            if !rest.tail.is_empty() {
                return unsupported();
            }
            let ty = view.type_token_node(rest.slot.ty)?;
            let ty = match rest.kind {
                crate::signature_kernel::RestKind::Array => {
                    self.graph().intern_node(SemanticNodeData::Array {
                        element: ty,
                        readonly: false,
                    })
                }
                crate::signature_kernel::RestKind::GenericTuple => ty,
            };
            params.push(crate::semantic_query::FunctionParam::synthetic(
                named(&rest.slot, None)?,
                ty,
                false,
                true,
            ));
        }
        Ok(Arc::from(params.into_boxed_slice()))
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The `execute(SignaturesOfType)` producer.
    pub(super) fn build_signatures_of_type(
        &self,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> super::walk::QueryBuildOutput<crate::semantic_query::SemanticQueryValue> {
        use crate::semantic_query::{QueryError, QueryResult, SemanticQueryValue};
        let fence = self.project_generation_signature();
        let store = self.graph().store().signature_store();
        let outcome = self.signatures_of_type_with_authored(store, subject, kind, context);
        #[cfg(any(test, feature = "test-support"))]
        signatures_of_type_build_point(SignaturesOfTypeBuildPoint::Settled);
        match outcome {
            QueryOutcome::Ready(Ready { value, .. }) => {
                let (roots, complete) =
                    match self.transitive_self_roots_from_nodes(std::iter::once(subject)) {
                        Ok(roots) => (roots, true),
                        Err(_) => (Vec::new(), false),
                    };
                let mut output = super::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::SignatureSet(value)),
                    fence,
                ))
                .with_observed_self_roots(roots);
                output.cache_suppress |= !complete;
                output
            }
            QueryOutcome::Incomplete(reason) => incomplete_output(fence, reason, QueryError::Miss),
        }
    }

    /// The `execute(ReadSignatureResult)` producer.
    pub(super) fn build_read_signature_result(
        &self,
        key: &crate::signature_kernel::ReadSignatureResultKey,
    ) -> super::walk::QueryBuildOutput<crate::semantic_query::SemanticQueryValue> {
        use crate::semantic_query::{QueryError, QueryResult, SemanticQueryValue};
        let fence = self.project_generation_signature();
        let store = self.graph().store().signature_store();
        // The read works in the epoch its key names, which must be the
        // current one: a key of a retired epoch is refused before any record
        // is read.
        let binding = match store.bind_epoch(key.descriptor.epoch()) {
            Ok(binding) => binding,
            Err(_) => {
                return incomplete_output(
                    fence,
                    DiscoveryError::Store(crate::signature_kernel::StoreError::StaleHandle)
                        .incomplete_reason(),
                    QueryError::Miss,
                )
            }
        };
        match self.read_signature_result(
            &binding,
            key.descriptor,
            key.call_substitution,
            key.projection,
            key.evaluation,
            key.semantic_context.clone(),
        ) {
            QueryOutcome::Ready(Ready { value, .. }) => {
                #[cfg(any(test, feature = "test-support"))]
                signatures_of_type_build_point(SignaturesOfTypeBuildPoint::ResultApplied);
                let return_node = binding
                    .view()
                    .applied_result(value)
                    .ok()
                    .and_then(|record| record.return_type)
                    .and_then(|token| binding.view().type_token_node(token).ok());
                let roots = self.observed_self_roots_from_nodes(return_node);
                let version = crate::signature_kernel::SignatureVersion(Some(binding.lease()));
                let mut output = super::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::SignatureResult(
                        crate::signature_kernel::SignatureResultValue {
                            result: value,
                            version,
                        },
                    )),
                    fence,
                ))
                .with_observed_self_roots(roots);
                if key.projection == ResultDemand::Return {
                    // A ready recipe visited every demanded return constituent;
                    // body constituents carry their independently evaluated
                    // return, including their anchored unresolved positions.
                    output = output.with_positional_guarantee();
                }
                output
            }
            QueryOutcome::Incomplete(reason) => incomplete_output(fence, reason, QueryError::Miss),
        }
    }
}

/// The windows of a `SignaturesOfType` list's life a kernel-epoch
/// replacement can land in: two inside its build, one after a consumer was
/// handed the completed list, and one after a composite candidate's result
/// was read for it.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignaturesOfTypeBuildPoint {
    /// The walk has discovered its candidates and not yet published the
    /// set: a replacement here retires what the walk interned.
    Discovered,
    /// The build has settled its value and not yet handed it to the memo: a
    /// replacement here makes the build publish a retired-epoch value.
    Settled,
    /// A consumer holds a completed list and is about to pin it: a
    /// replacement here retires a list that was current when it completed.
    Pinning,
    /// A `ReadSignatureResult` build has its applied result and has not yet
    /// handed it to the memo: a replacement here retires the epoch the
    /// result was published in before its reader consumes it.
    ResultApplied,
}

#[cfg(any(test, feature = "test-support"))]
type SignaturesOfTypeBuildHook = Box<dyn Fn(SignaturesOfTypeBuildPoint)>;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Test-only: runs on the building thread at each
    /// [`SignaturesOfTypeBuildPoint`].
    pub static SIGNATURES_OF_TYPE_BUILD_HOOK: std::cell::RefCell<Option<SignaturesOfTypeBuildHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
fn signatures_of_type_build_point(point: SignaturesOfTypeBuildPoint) {
    SIGNATURES_OF_TYPE_BUILD_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(point);
        }
    });
}

fn incomplete_output(
    fence: crate::semantic_query::DepSignature,
    reason: IncompleteReason,
    error: crate::semantic_query::QueryError,
) -> super::walk::QueryBuildOutput<crate::semantic_query::SemanticQueryValue> {
    let mut output = super::walk::QueryBuildOutput::from((
        crate::semantic_query::QueryResult::Error(error),
        fence,
    ));
    output.cache_suppress = true;
    output.fold_partial(matches!(
        reason,
        IncompleteReason::Budget | IncompleteReason::Cancelled
    ));
    output
}

#[cfg(test)]
#[path = "signature_rootless_space_tests.rs"]
mod signature_rootless_space_tests;
