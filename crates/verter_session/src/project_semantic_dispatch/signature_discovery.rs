//! Graph-facing signature discovery: `SignaturesOfType` and
//! `ReadSignatureResult`.
//!
//! The subject walk settles a type to its signature-bearing shape (carriers
//! and aliases through the shared settlement rail, unions through the
//! `VerterStableV1` arm order, intersections in authored order, constrained
//! type parameters through their constraint, apparent primitives and
//! collections through the resolved global population) and publishes the
//! candidates through the record-level discovery in
//! [`crate::signature_kernel`]. Enumeration never reads a body: a body
//! return is a closed recipe until [`ProjectSemanticDispatch::read_signature_result`]
//! forces it.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rustc_hash::{FxHashSet, FxHasher};

use crate::semantic_query::{
    CanonicalTypeSubstitution, IncompleteReason, LiteralValue, ProjectionReductionContext,
    QueryOutcome, Ready, ResolveOverloadSetConsumer, ResultEvaluationContextId, SemanticContext,
    SemanticContextId, SemanticNodeData, SemanticNodeId, SignatureKind as GraphSignatureKind,
    CONTEXT_FREE_EVALUATION, CONTEXT_FREE_EVIDENCE,
};
use crate::signature_kernel::{
    intersection_signatures, publish_signature, set_from_candidates, union_signatures,
    AppliedResult, AppliedResultId, BinderInput, DeclarationGroupId, DeclarationParentId,
    DiscoveryError, DiscoveryTypes, ParamInput, RestInput, ResultDemand, ResultInput,
    SemanticReadView, SignatureCandidate, SignatureDescriptorId, SignatureInput, SignatureKind,
    SignatureProvenance, SignatureResultRecipe, SignatureSemanticFlags, SignatureStore,
    SlotTypeFacts, SourceLocatorId, TypeToken,
};
use verter_semantic::analysis::type_solver::arena::PrimitiveKind;

use super::ProjectSemanticDispatch;

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
fn space_key_of<T: Hash>(value: &T) -> u64 {
    hash_u64(value) & 0x7FFF_FFFF
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

/// The graph adapter the record-level discovery reads types through.
pub(crate) struct GraphTypes<'a, 'd> {
    dispatch: &'a ProjectSemanticDispatch<'d>,
    store: &'a SignatureStore,
}

impl<'a, 'd> GraphTypes<'a, 'd> {
    fn node(&self, token: TypeToken) -> Option<SemanticNodeId> {
        self.store.type_token_node(token).ok()
    }

    fn token(&self, node: SemanticNodeId) -> Option<TypeToken> {
        self.store.intern_type_token(node, None).ok()
    }
}

impl SlotTypeFacts for GraphTypes<'_, '_> {
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

impl DiscoveryTypes for GraphTypes<'_, '_> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        let na = self.node(a)?;
        let nb = self.node(b)?;
        let moving: Vec<(SemanticNodeId, SemanticNodeId)> =
            corr.iter().copied().filter(|(x, y)| x != y).collect();
        let na = if moving.is_empty() {
            na
        } else {
            self.dispatch
                .substitute_canonical(na, &CanonicalTypeSubstitution::new(moving))
        };
        if na == nb {
            return Some(true);
        }
        let graph = self.dispatch.graph();
        let ka = crate::semantic_query::stable_key::stable_key_for_node(graph, na);
        let kb = crate::semantic_query::stable_key::stable_key_for_node(graph, nb);
        if !ka.is_complete() || !kb.is_complete() {
            return None;
        }
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
        let mapped = self
            .dispatch
            .substitute_canonical(node, &CanonicalTypeSubstitution::new(map.to_vec()));
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

    fn forced_return(&self, candidate: &SignatureCandidate) -> Result<TypeToken, IncompleteReason> {
        let empty = self
            .store
            .intern_substitution(
                crate::signature_kernel::CallSubstitution::identity(
                    self.residual_space(candidate.signature)
                        .ok_or(IncompleteReason::UnsettledInput)?,
                ),
                None,
            )
            .map_err(|_| IncompleteReason::UnsettledInput)?;
        let result = self.dispatch.read_signature_result(
            self.store,
            candidate.signature,
            empty,
            ResultDemand::Return,
            CONTEXT_FREE_EVALUATION,
            SemanticContextId::production(),
        );
        match result {
            QueryOutcome::Ready(Ready { value, .. }) => self
                .store
                .applied_result(value)
                .ok()
                .and_then(|r| r.return_type)
                .ok_or(IncompleteReason::UnsettledInput),
            QueryOutcome::Incomplete(reason) => Err(reason),
        }
    }
}

impl GraphTypes<'_, '_> {
    fn residual_space(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Option<crate::signature_kernel::BinderSpaceId> {
        let view = SemanticReadView::pin(self.store);
        view.descriptor(descriptor).ok().map(|d| d.residual_binders)
    }
}

type Found = Result<Vec<SignatureCandidate>, DiscoveryError>;

fn unsettled<T>() -> Result<T, DiscoveryError> {
    Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))
}

fn unsupported<T>() -> Result<T, DiscoveryError> {
    Err(DiscoveryError::Incomplete(IncompleteReason::Unsupported))
}

struct Walk<'w, 'a, 'd> {
    types: &'w GraphTypes<'a, 'd>,
    kind: GraphSignatureKind,
    context_id: SemanticContextId,
    context: Option<SemanticContext>,
    visiting: FxHashSet<SemanticNodeId>,
    /// The authored node each leaf candidate was published from, first
    /// publication wins within this walk.
    authored: rustc_hash::FxHashMap<SignatureDescriptorId, SemanticNodeId>,
}

impl<'w, 'a, 'd> Walk<'w, 'a, 'd> {
    fn dispatch(&self) -> &'a ProjectSemanticDispatch<'d> {
        self.types.dispatch
    }

    fn strict_null(&self) -> bool {
        self.context
            .as_ref()
            .is_none_or(|c| c.effective_semantic_options.strict_null_checks)
    }

    fn discover(&mut self, node: SemanticNodeId) -> Found {
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
            SemanticNodeData::MergedDecl { .. } => {
                drop(data);
                match self.dispatch().unwrap_identity_carrier_for_relation(node) {
                    super::relation::IdentityCarrierUnwrap::Concrete(settled)
                        if settled != node =>
                    {
                        self.discover(settled)
                    }
                    _ => unsettled(),
                }
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
                let mut out = Vec::with_capacity(list.len());
                for (ordinal, sig) in list.iter().enumerate() {
                    let sig = self.dispatch().resolve_signature_source_carrier(
                        *sig,
                        ProjectionReductionContext::structural_transit(),
                    );
                    match self.leaf(sig, ordinal as u32)? {
                        Some(candidate) => out.push(candidate),
                        None => return unsupported(),
                    }
                }
                Ok(out)
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
                let context = self
                    .context
                    .clone()
                    .unwrap_or_else(SemanticContext::production);
                let arms = crate::semantic_query::stable_key::semantic_union_members(
                    graph, node, &context,
                );
                let mut lists = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    lists.push(self.discover(*arm)?);
                }
                union_signatures(self.types.store, self.types, &lists)
            }
            SemanticNodeData::Intersection(members) => {
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                let mut lists = Vec::with_capacity(members.len());
                for member in members {
                    lists.push(self.discover(member)?);
                }
                intersection_signatures(
                    self.types.store,
                    self.types,
                    kernel_kind(self.kind),
                    &lists,
                )
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
                (Arc::clone(&identity.decl_name), Vec::new())
            }
            Some(SemanticNodeData::InstantiationRef { base, args })
                if d.runtime_nominal_identity(base).is_some() =>
            {
                (Arc::clone(&base.decl_name), args.iter().copied().collect())
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
            crate::file_artifact_store::AugmentationTargetKind::GlobalAugmentation,
            name,
            type_arguments,
            ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Expanded),
            request_canonical.as_deref().unwrap_or(""),
        ) else {
            return Ok(Vec::new());
        };
        if contributions.source_env_unobservable {
            d.fold_into_top_build_local_taint(false, true);
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::UnobservableSource,
            );
        }
        let mut out = Vec::new();
        for contributor in contributions.contributor_nodes {
            out.extend(self.discover(contributor)?);
        }
        Ok(out)
    }

    /// Apparent-type signatures of a global wrapper interface, read from the
    /// resolved global population of the request's project. An absent global
    /// (`noLib`) is the checker's empty apparent type: a complete negative.
    fn apparent(&mut self, name: &str, args: &[SemanticNodeId]) -> Found {
        let d = self.dispatch();
        let Some(canonical) = crate::request_context::current_request_canonical()
            .or_else(|| d.lexical_demand_scope.borrow().last().cloned())
        else {
            return unsettled();
        };
        let Some(project) = d.project_stable_key_for_canonical(canonical.as_ref()) else {
            return unsettled();
        };
        let Some(hit) = d.ctx.lookup_ambient_symbol(project, name) else {
            return Ok(Vec::new());
        };
        d.ctx
            .record_ambient_dependency(canonical.as_ref(), hit.virtual_id.as_ref());
        let slot = d.type_slot_for(
            Arc::clone(&hit.virtual_id),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
        );
        let surface = d.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                Arc::from(args.to_vec().into_boxed_slice()),
                d.instantiate_context_for(
                    hit.virtual_id.as_ref(),
                    ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                ),
            ),
        ));
        match surface.value {
            crate::semantic_query::QueryResult::Value(surface) => self.discover(surface),
            _ => unsettled(),
        }
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
        let (kind, params, type_parameters, occurrence, carrier, span) = match &*data {
            SemanticNodeData::Signature {
                kind,
                params,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                ..
            } => (
                *kind,
                Arc::clone(params),
                Arc::clone(type_parameters),
                occurrence.clone(),
                return_carrier.clone(),
                *signature_span,
            ),
            SemanticNodeData::DeferredCallable(callable) => {
                let parts = callable.parts(&ResolveOverloadSetConsumer::witness());
                (
                    parts.kind,
                    Arc::clone(parts.params),
                    Arc::clone(parts.type_parameters),
                    Some(parts.occurrence.clone()),
                    parts.return_carrier.clone(),
                    None,
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
            name: p.name.clone(),
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
                        name: param.name.clone(),
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
                            name: element.label.clone(),
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
                            name: param.name.clone(),
                            ty: element,
                            generic: false,
                            tail,
                        });
                    }
                }
                _ => {
                    rest = Some(RestInput {
                        name: param.name.clone(),
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
                name: Arc::clone(&decl.name),
                param: decl.param,
                constraint: decl.constraint,
                default: decl.default,
            })
            .collect();
        let result = match &carrier {
            crate::semantic_query::SignatureReturnCarrier::Declared(return_type) => {
                ResultInput::Declared {
                    return_type: *return_type,
                    predicate_or_assertion: None,
                }
            }
            crate::semantic_query::SignatureReturnCarrier::Function(source) => ResultInput::Body {
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
                let key = crate::semantic_query::stable_key::stable_key_for_node(graph, node);
                // Only a complete key proves which binder space a rootless
                // signature's binders live in. A signature with no binders
                // mints no binder token, so an over-deep structure still
                // publishes: two of them sharing the depth-exhausted key
                // share an EMPTY space, which identifies nothing.
                if !key.is_complete() && !type_parameters.is_empty() {
                    return unsettled();
                }
                let fingerprint = key.fingerprint();
                (
                    space_key_of(&fingerprint),
                    identity_u128(&fingerprint),
                    hash_u64(&fingerprint),
                    0,
                    ordinal,
                )
            }
        };
        // An authored JavaScript signature with no type information at all:
        // every parameter is implicitly `any` and the result is inferred.
        let untyped_js = occurrence.as_ref().is_some_and(|occ| {
            is_js_canonical(&occ.function.anchor.canonical_id)
                && matches!(
                    carrier,
                    crate::semantic_query::SignatureReturnCarrier::Function(_)
                )
        }) && params.iter().all(|p| {
            matches!(
                graph.node_data(p.ty).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
            )
        });
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
            flags: if untyped_js {
                SignatureSemanticFlags::UNTYPED_JS
            } else {
                SignatureSemanticFlags::NONE
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
        let candidate = publish_signature(self.types.store, &input)?;
        self.authored.entry(candidate.signature).or_insert(node);
        Ok(Some(candidate))
    }
}

impl ProjectSemanticDispatch<'_> {
    /// `SignaturesOfType(subject, kind, context)`: the ordered call or
    /// construct candidates of `subject`. An empty set is a complete
    /// negative; every failure to settle the subject is an explicit
    /// incomplete reason. The set alone; production reads
    /// [`Self::signatures_of_type_with_authored`].
    #[cfg(test)]
    pub(crate) fn signatures_of_type(
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
    pub(crate) fn signatures_of_type_with_authored(
        &self,
        store: &SignatureStore,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> QueryOutcome<crate::signature_kernel::SignatureSetValue> {
        let types = GraphTypes {
            dispatch: self,
            store,
        };
        let mut walk = Walk {
            types: &types,
            kind,
            context_id: context,
            context: context.lookup(),
            visiting: FxHashSet::default(),
            authored: rustc_hash::FxHashMap::default(),
        };
        let _ = walk.context_id;
        let found = walk.discover(subject);
        let published = found.and_then(|list| {
            let view = SemanticReadView::pin(store);
            let mut nodes_of: Vec<crate::signature_kernel::SignatureCandidateNodes> =
                Vec::with_capacity(list.len());
            for candidate in &list {
                let nodes = match composite_sequence(&view, candidate.signature)? {
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
                    constituents: Arc::from(nodes.into_boxed_slice()),
                });
            }
            drop(view);
            Ok((set_from_candidates(store, list)?, nodes_of))
        });
        match published {
            Ok((set, nodes_of)) => QueryOutcome::Ready(Ready {
                value: crate::signature_kernel::SignatureSetValue {
                    set,
                    nodes: Arc::from(nodes_of.into_boxed_slice()),
                },
                evidence: CONTEXT_FREE_EVIDENCE,
            }),
            Err(error) => QueryOutcome::Incomplete(error.incomplete_reason()),
        }
    }

    /// `ReadSignatureResult(descriptor, call substitution, demand,
    /// evaluation, context)`: force the demanded half of a candidate's
    /// result. The declaration environment and the call substitution compose
    /// into ONE map applied once; a body recipe is forced through the
    /// existing flow-return obligations; enumeration alone never gets here.
    pub(crate) fn read_signature_result(
        &self,
        store: &SignatureStore,
        descriptor: SignatureDescriptorId,
        call_substitution: crate::signature_kernel::CallSubstitutionId,
        demand: ResultDemand,
        evaluation: ResultEvaluationContextId,
        context: SemanticContextId,
    ) -> QueryOutcome<AppliedResultId> {
        let types = GraphTypes {
            dispatch: self,
            store,
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
        types: &GraphTypes<'_, '_>,
        descriptor: SignatureDescriptorId,
        call_substitution: crate::signature_kernel::CallSubstitutionId,
        demand: ResultDemand,
        evaluation: ResultEvaluationContextId,
        context: SemanticContextId,
    ) -> Result<AppliedResultId, DiscoveryError> {
        let store = types.store;
        let view = SemanticReadView::pin(store);
        let desc = *view.descriptor(descriptor)?;
        let template = *view.template(desc.template)?;
        let recipe = *view.recipe(template.result_recipe)?;
        let return_type = if demand.reads_return() {
            let node = self.recipe_return(
                types,
                &view,
                descriptor,
                call_substitution,
                evaluation,
                &recipe,
            )?;
            Some(store.intern_type_token(node, None)?)
        } else {
            None
        };
        let effects = if demand.reads_effects() {
            self.recipe_effects(types, &view, descriptor, call_substitution, &recipe)?
                .map(|node| store.intern_type_token(node, None))
                .transpose()?
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
        let raw = store.publish_result(record, None)?;
        Ok(AppliedResultId::from_raw(raw))
    }

    fn recipe_effects(
        &self,
        types: &GraphTypes<'_, '_>,
        view: &SemanticReadView,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
        recipe: &SignatureResultRecipe,
    ) -> Result<Option<SemanticNodeId>, DiscoveryError> {
        match recipe {
            SignatureResultRecipe::Declared {
                predicate_or_assertion: Some(token),
                ..
            } => {
                let base = view.type_token_node(*token)?;
                let map = self.composed_map(types.store, view, descriptor, call)?;
                Ok(Some(self.substitute_canonical(base, &map)))
            }
            SignatureResultRecipe::Declared {
                predicate_or_assertion: None,
                ..
            } => Ok(None),
            SignatureResultRecipe::Body { .. }
            | SignatureResultRecipe::UnionCommon { .. }
            | SignatureResultRecipe::UnionSynthesized { .. }
            | SignatureResultRecipe::IntersectionConstruct { .. } => Err(
                DiscoveryError::Incomplete(IncompleteReason::UnresolvedObligation),
            ),
        }
    }

    /// The single composed map for `descriptor` under `call`: declaration
    /// environment first, call substitution second, dropping any binding
    /// whose image is still an unresolved residual binder token.
    fn composed_map(
        &self,
        store: &SignatureStore,
        view: &SemanticReadView,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, DiscoveryError> {
        let desc = view.descriptor(descriptor)?;
        let env = view.environment(desc.declaration_environment)?;
        let call_map = store.flatten_substitution(call)?;
        let composed = crate::signature_kernel::compose_canonical(env, &call_map);
        let kept: Vec<(SemanticNodeId, SemanticNodeId)> = composed
            .bindings()
            .iter()
            .copied()
            .filter(|(_, image)| !SignatureStore::is_binder_token(*image))
            .collect();
        Ok(CanonicalTypeSubstitution::new(kept))
    }

    fn recipe_return(
        &self,
        types: &GraphTypes<'_, '_>,
        view: &SemanticReadView,
        descriptor: SignatureDescriptorId,
        call: crate::signature_kernel::CallSubstitutionId,
        evaluation: ResultEvaluationContextId,
        recipe: &SignatureResultRecipe,
    ) -> Result<SemanticNodeId, DiscoveryError> {
        let store = types.store;
        match recipe {
            SignatureResultRecipe::Declared { return_type, .. } => {
                let base = view.type_token_node(*return_type)?;
                let map = self.composed_map(store, view, descriptor, call)?;
                Ok(self.substitute_canonical(base, &map))
            }
            SignatureResultRecipe::Body { .. } => {
                store.note_body_forced();
                let Some(source) = store.descriptor_source(descriptor)? else {
                    return Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput));
                };
                let node = view.type_token_node(source)?;
                let identity = {
                    let graph = self.graph();
                    let data = graph.node_data(node);
                    match data.as_deref() {
                        Some(SemanticNodeData::Signature {
                            return_carrier:
                                crate::semantic_query::SignatureReturnCarrier::Function(
                                    verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                                ),
                            ..
                        }) => identity.clone(),
                        Some(SemanticNodeData::DeferredCallable(callable)) => {
                            match callable
                                .parts(&ResolveOverloadSetConsumer::witness())
                                .return_carrier
                            {
                                crate::semantic_query::SignatureReturnCarrier::Function(
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
                let map = self.composed_map(store, view, descriptor, call)?;
                let unknown = self
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
                let desc = view.descriptor(descriptor)?;
                let env = view.environment(desc.declaration_environment)?;
                let mut params: Vec<SemanticNodeId> =
                    env.bindings().iter().map(|(p, _)| *p).collect();
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
                let key = self.flow_return_key_for_instantiation_with_evaluation(
                    &identity,
                    Arc::from(args.into_boxed_slice()),
                    map,
                    evaluation,
                );
                match self.execute_flow_return(key) {
                    crate::semantic_query::FlowReturnStep::Complete(result) => {
                        Ok(result.return_type())
                    }
                    crate::semantic_query::FlowReturnStep::NoValue(
                        crate::semantic_query::FlowReturnFailure::Budget(_),
                    ) => Err(DiscoveryError::Incomplete(IncompleteReason::Budget)),
                    crate::semantic_query::FlowReturnStep::Hold(_)
                    | crate::semantic_query::FlowReturnStep::NoValue(_) => Err(
                        DiscoveryError::Incomplete(IncompleteReason::UnresolvedObligation),
                    ),
                }
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
pub(super) enum PositionalArgument {
    /// The candidate declares nothing at that position and has no rest run.
    Absent,
    Type {
        /// The declared type at the position.
        ty: SemanticNodeId,
        /// Its union arms without `null` / `undefined`, in union order; a
        /// non-union is its own single arm, and an all-nullish type is the
        /// empty list.
        non_nullish_arms: Vec<SemanticNodeId>,
    },
}

/// One candidate's positional slots as read under the pinned view, before
/// any semantics is dispatched over them.
struct RawPositional {
    receiver: Option<SemanticNodeId>,
    /// `None` when the candidate declares nothing at the position.
    argument: Option<SemanticNodeId>,
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

/// The ordered shared candidates of one subject as graph signature nodes.
pub(super) enum SharedSignatureNodes {
    /// Every candidate, in candidate order. Empty is a complete negative.
    Nodes(Vec<SemanticNodeId>),
    /// The subject did not settle, or a candidate has no node form.
    Incomplete(IncompleteReason),
}

impl ProjectSemanticDispatch<'_> {
    /// The subject's candidates of `kind`, read from `SignaturesOfType`, each
    /// as the graph signature node a node-based consumer reads: the authored
    /// node for a leaf, and for a composite the node form of its descriptor
    /// (the kernel's parameter layout and binders, with the return read
    /// through `ReadSignatureResult`). No signature is chosen, merged or
    /// reordered here.
    pub(super) fn shared_signature_nodes(
        &self,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
    ) -> SharedSignatureNodes {
        use crate::semantic_query::{QueryResult, SemanticQueryKey, SemanticQueryValue};
        let read = self.execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
            subject,
            kind,
            context: SemanticContextId::production(),
        });
        let value = match read.value {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => value,
            _ => {
                return SharedSignatureNodes::Incomplete(if self.connected_demand_tripped() {
                    IncompleteReason::Budget
                } else {
                    IncompleteReason::UnsettledInput
                })
            }
        };
        let store = self.graph().signature_store();
        let candidates: Vec<SignatureCandidate> = {
            let view = SemanticReadView::pin(store);
            match view.read_set(value.set) {
                Ok(crate::signature_kernel::BorrowedSet::Empty) => Vec::new(),
                Ok(crate::signature_kernel::BorrowedSet::One { candidate, .. }) => vec![candidate],
                Ok(crate::signature_kernel::BorrowedSet::Many(list)) => list.to_vec(),
                Err(_) => {
                    return SharedSignatureNodes::Incomplete(IncompleteReason::UnsettledInput)
                }
            }
        };
        if candidates.len() != value.nodes.len() {
            return SharedSignatureNodes::Incomplete(IncompleteReason::UnsettledInput);
        }
        let mut nodes = Vec::with_capacity(candidates.len());
        for (index, candidate) in candidates.iter().enumerate() {
            let node = match value.nodes[index].authored {
                Some(node) => node,
                None => match self.composite_signature_node(
                    store,
                    kind,
                    candidate.signature,
                    &value.nodes[index].constituents,
                ) {
                    Ok(node) => node,
                    Err(error) => {
                        return SharedSignatureNodes::Incomplete(error.incomplete_reason())
                    }
                },
            };
            nodes.push(node);
        }
        SharedSignatureNodes::Nodes(nodes)
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
        use crate::semantic_query::{QueryResult, SemanticQueryKey, SemanticQueryValue};
        use crate::signature_kernel::{PositionalShape, TypeAt};

        let read = self.execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
            subject,
            kind,
            context: SemanticContextId::production(),
        });
        let value = match read.value {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => value,
            _ => {
                return Err(if self.connected_demand_tripped() {
                    IncompleteReason::Budget
                } else {
                    IncompleteReason::UnsettledInput
                })
            }
        };
        let store = self.graph().signature_store();
        // Everything the positional model needs is read under ONE pinned
        // view; no semantics is dispatched while it is held.
        let raw: Result<Vec<RawPositional>, ()> = {
            let view = SemanticReadView::pin(store);
            let candidates: Vec<SignatureCandidate> = match view.read_set(value.set) {
                Ok(crate::signature_kernel::BorrowedSet::Empty) => Vec::new(),
                Ok(crate::signature_kernel::BorrowedSet::One { candidate, .. }) => vec![candidate],
                Ok(crate::signature_kernel::BorrowedSet::Many(list)) => list.to_vec(),
                Err(_) => return Err(IncompleteReason::UnsettledInput),
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
                        Ok(node) => Some(node),
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
        let Ok(raw) = raw else {
            return Err(IncompleteReason::UnsettledInput);
        };
        Ok(raw
            .into_iter()
            .map(|raw| PositionalRead {
                receiver: raw.receiver,
                argument: match raw.argument {
                    None => PositionalArgument::Absent,
                    Some(ty) => PositionalArgument::Type {
                        ty,
                        non_nullish_arms: self.non_nullish_arms(ty),
                    },
                },
            })
            .collect())
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

    /// Both buckets of the subject's shared list as signature nodes, call
    /// then construct; the reason either list did not settle otherwise.
    pub(super) fn shared_signature_buckets(
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

    /// The parameters and binders an authored signature node carries.
    fn authored_signature_parts(&self, node: SemanticNodeId) -> Option<AuthoredSignatureParts> {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                type_parameters,
                ..
            }) => Some((Arc::clone(params), Arc::clone(type_parameters))),
            Some(SemanticNodeData::DeferredCallable(callable)) => {
                let parts = callable.parts(&ResolveOverloadSetConsumer::witness());
                Some((Arc::clone(parts.params), Arc::clone(parts.type_parameters)))
            }
            _ => None,
        }
    }

    /// The graph node form of one composite descriptor. `constituent_nodes`
    /// are the authored nodes of its constituents, in sequence order.
    fn composite_signature_node(
        &self,
        store: &SignatureStore,
        kind: GraphSignatureKind,
        descriptor: SignatureDescriptorId,
        constituent_nodes: &[SemanticNodeId],
    ) -> Result<SemanticNodeId, DiscoveryError> {
        let view = SemanticReadView::pin(store);
        let desc = *view.descriptor(descriptor)?;
        let template = *view.template(desc.template)?;
        let shape = *view.shape(template.input_shape)?;
        let Some(sequence) = composite_sequence(&view, descriptor)? else {
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
                None => self.layout_params(&view, &shape)?,
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
        let call = store.intern_substitution(
            crate::signature_kernel::CallSubstitution::map(
                desc.residual_binders,
                CanonicalTypeSubstitution::new(naming),
            ),
            None,
        )?;
        drop(view);
        let return_type = match self.read_signature_result(
            store,
            descriptor,
            call,
            ResultDemand::Return,
            CONTEXT_FREE_EVALUATION,
            SemanticContextId::production(),
        ) {
            QueryOutcome::Ready(Ready { value, .. }) => {
                let token = store
                    .applied_result(value)?
                    .return_type
                    .ok_or(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))?;
                store.type_token_node(token)?
            }
            QueryOutcome::Incomplete(reason) => return Err(DiscoveryError::Incomplete(reason)),
        };
        Ok(self.graph().intern_node(SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            type_parameters,
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(return_type),
            signature_span: None,
            return_type_span: None,
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

impl ProjectSemanticDispatch<'_> {
    /// The `execute(SignaturesOfType)` producer.
    pub(super) fn build_signatures_of_type(
        &self,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> super::walk::QueryBuildOutput<crate::semantic_query::SemanticQueryValue> {
        use crate::semantic_query::{QueryError, QueryResult, SemanticQueryValue};
        let fence = self.project_generation_signature();
        match self.signatures_of_type_with_authored(
            self.graph().signature_store(),
            subject,
            kind,
            context,
        ) {
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
        let store = self.graph().signature_store();
        match self.read_signature_result(
            store,
            key.descriptor,
            key.call_substitution,
            key.projection,
            key.evaluation,
            key.semantic_context,
        ) {
            QueryOutcome::Ready(Ready { value, .. }) => {
                let return_node = store
                    .applied_result(value)
                    .ok()
                    .and_then(|record| record.return_type)
                    .and_then(|token| store.type_token_node(token).ok());
                let roots = self.observed_self_roots_from_nodes(return_node);
                super::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::SignatureResult(
                        crate::signature_kernel::SignatureResultValue { result: value },
                    )),
                    fence,
                ))
                .with_observed_self_roots(roots)
            }
            QueryOutcome::Incomplete(reason) => incomplete_output(fence, reason, QueryError::Miss),
        }
    }
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
    output.result_is_partial = matches!(
        reason,
        IncompleteReason::Budget | IncompleteReason::Cancelled
    );
    output
}
