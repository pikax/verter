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
    SignatureProvenance, SignatureResultRecipe, SignatureSemanticFlags, SignatureSetRef,
    SignatureStore, SlotTypeFacts, SourceLocatorId, TypeToken,
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

fn hash_u32<T: Hash>(value: &T) -> u32 {
    let h = hash_u64(value);
    (h ^ (h >> 32)) as u32
}

/// `VerterStableV1`-independent, order-independent binder-space key: derived
/// from logical identity, never from intern order.
fn space_key_of<T: Hash>(value: &T) -> u64 {
    hash_u64(value) & 0x7FFF_FFFF
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

    fn unknown(&self) -> TypeToken {
        let node = self
            .dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        self.token(node).expect("token intern on a live epoch")
    }

    fn any(&self) -> TypeToken {
        let node = self
            .dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
        self.token(node).expect("token intern on a live epoch")
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
                }
            }
            crate::semantic_query::SignatureReturnCarrier::Function(source) => ResultInput::Body {
                locator: hash_u64(source),
            },
        };
        let (space_key, group, parent, source_ordinal) = match &occurrence {
            Some(occ) => (
                space_key_of(&(&occ.function, occ.signature_ordinal)),
                hash_u32(&occ.function),
                hash_u32(&occ.function.anchor.canonical_id),
                occ.signature_ordinal,
            ),
            None => {
                let key = crate::semantic_query::stable_key::stable_key_for_node(graph, node);
                let fingerprint = key.fingerprint();
                (
                    space_key_of(&fingerprint),
                    hash_u32(&fingerprint),
                    0,
                    ordinal,
                )
            }
        };
        let locator = span.map_or(0, |s| hash_u32(&(s.start, s.end)));
        let input = SignatureInput {
            kind: kernel_kind(kind),
            source: node,
            space_key,
            binders,
            receiver: receiver.map(to_param),
            params: fixed,
            rest,
            flags: SignatureSemanticFlags::NONE,
            result,
            provenance: SignatureProvenance::authored(
                DeclarationGroupId::from_raw(group),
                DeclarationParentId::from_raw(parent),
                source_ordinal,
                ordinal,
                SourceLocatorId::from_raw(locator),
            ),
        };
        publish_signature(self.types.store, &input).map(Some)
    }
}

impl ProjectSemanticDispatch<'_> {
    /// `SignaturesOfType(subject, kind, context)`: the ordered call or
    /// construct candidates of `subject`. An empty set is a complete
    /// negative; every failure to settle the subject is an explicit
    /// incomplete reason.
    pub(crate) fn signatures_of_type(
        &self,
        store: &SignatureStore,
        subject: SemanticNodeId,
        kind: GraphSignatureKind,
        context: SemanticContextId,
    ) -> QueryOutcome<SignatureSetRef> {
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
        };
        let _ = walk.context_id;
        let found = walk.discover(subject);
        match found.and_then(|list| set_from_candidates(store, list)) {
            Ok(set) => QueryOutcome::Ready(Ready {
                value: set,
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
        if !evaluation.is_context_free() {
            return QueryOutcome::Incomplete(IncompleteReason::Unsupported);
        }
        let types = GraphTypes {
            dispatch: self,
            store,
        };
        match self.read_result_inner(&types, descriptor, call_substitution, demand, context) {
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
        context: SemanticContextId,
    ) -> Result<AppliedResultId, DiscoveryError> {
        let store = types.store;
        let view = SemanticReadView::pin(store);
        let desc = *view.descriptor(descriptor)?;
        let template = *view.template(desc.template)?;
        let recipe = view.recipe(template.result_recipe)?.clone();
        let return_type = if demand.reads_return() {
            let node = self.recipe_return(types, &view, descriptor, call_substitution, &recipe)?;
            Some(store.intern_type_token(node, None)?)
        } else {
            None
        };
        let record = AppliedResult {
            descriptor,
            substitution: call_substitution,
            recipe: template.result_recipe,
            evaluation: CONTEXT_FREE_EVALUATION,
            semantic_context: context,
            evidence: CONTEXT_FREE_EVIDENCE,
            return_type,
            effects: None,
        };
        let raw = store.publish_result(record, None)?;
        Ok(AppliedResultId::from_raw(raw))
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
                        .map_or(0, |(_, token)| token.0 as u32)
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
                let key = self.flow_return_key_for_instantiation(
                    &identity,
                    Arc::from(args.into_boxed_slice()),
                    map,
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
                    let recipe = view.recipe(template.result_recipe)?.clone();
                    returns.push(self.recipe_return(types, view, edge.residual, call, &recipe)?);
                }
                let is_union =
                    !matches!(recipe, SignatureResultRecipe::IntersectionConstruct { .. });
                Ok(self.intern_normalized_union_or_intersection(&returns, is_union))
            }
        }
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
        match self.signatures_of_type(self.graph().signature_store(), subject, kind, context) {
            QueryOutcome::Ready(Ready { value, .. }) => {
                let (roots, complete) =
                    match self.transitive_self_roots_from_nodes(std::iter::once(subject)) {
                        Ok(roots) => (roots, true),
                        Err(_) => (Vec::new(), false),
                    };
                let mut output = super::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::SignatureSet(
                        crate::signature_kernel::SignatureSetValue { set: value },
                    )),
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
                let roots = self.observed_self_roots_from_nodes(return_node.into_iter());
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
