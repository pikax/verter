use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::intrinsic_registry::RuntimeNominal;
use crate::locator_identity::{BroadRuntimeSubjectLocator, BroadRuntimeSubjectRoute};
use crate::semantic_query::{
    BroadRuntimeClassification, BroadRuntimeKind, CacheRead, DepSignature, LiteralValue,
    PartialReasonSet, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryValue,
    SurfaceProvenanceContext,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RuntimeWork {
    node: SemanticNodeId,
    filter_unknown: bool,
}

struct RehydratedRuntimeSubject {
    node: SemanticNodeId,
    dep_signature: DepSignature,
    observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    cache_suppress: bool,
    result_is_partial: bool,
    /// The classes behind [`Self::result_is_partial`]. The boolean alone
    /// cannot distinguish a partial that leaves the resolved SHAPE intact
    /// from one that does not, and that distinction is what decides
    /// whether the addressed MEMBER can be projected at all.
    partial_reasons: PartialReasonSet,
}

/// What an observed partial says about the subject the classifier is about
/// to read — the split the raw `result_is_partial` boolean cannot express.
///
/// A flow-return degradation is a statement about a VALUE inside a surface
/// whose structure the substrate did produce. Every other class
/// (a budget, a cancellation, a torn view, a recursion, a missing node) is
/// a statement about the demand itself, and leaves nothing trustworthy.
///
/// Both fields stay independent of `result_is_partial`, which remains
/// `true` for every one of these: a degraded result is still `ReturnOnly`
/// and never warms.
#[derive(Clone, Copy)]
struct ObservedRuntimePartial {
    /// Whether the resolved node is the structure the demand asked for, so
    /// the addressed member can be selected out of it. True for both
    /// flow-return classes: the UNINFERRED class carries the typed marker
    /// at the one position it could not type and leaves every sibling
    /// exact, and the UNVERIFIED class has a COMPLETE member set by
    /// definition.
    shape_is_trustworthy: bool,
    /// Whether a member's own node may be classified into runtime
    /// constructors. False as soon as a class says a member's TYPE may be
    /// silently WRONG — `FLOW_RETURN_UNVERIFIED` is exactly that claim,
    /// and it is a property of the FRAME (seeded from the lowered slice's
    /// effect list before any member evaluates), so it disqualifies every
    /// member rather than a nameable one.
    ///
    /// The UNINFERRED class does NOT disqualify: its untyped position
    /// carries the typed marker, so the member holding it classifies
    /// itself as unknown while its exact siblings classify exactly.
    member_types_are_trustworthy: bool,
}

impl ObservedRuntimePartial {
    const COMPLETE: Self = Self {
        shape_is_trustworthy: true,
        member_types_are_trustworthy: true,
    };

    /// The NOTHING-SURVIVES verdict — a partial about the demand itself.
    const OPAQUE: Self = Self {
        shape_is_trustworthy: false,
        member_types_are_trustworthy: false,
    };

    fn observe(result_is_partial: bool, reasons: PartialReasonSet) -> Self {
        if !result_is_partial {
            return Self::COMPLETE;
        }
        // A partial that named NO class is not a licence: "empty" is
        // vacuously a subset of every set, so reading it structurally would
        // make an unclassified partial the most trustworthy value here
        // instead of the least. Callers reach this through
        // `CacheRead::partial_reason_classes`, which substitutes the
        // `PROPAGATED` bridge, so the arm should be unreachable — which is
        // exactly why it must fail closed rather than fail open.
        if reasons.is_empty() {
            return Self::OPAQUE;
        }
        Self {
            shape_is_trustworthy: reasons
                .without(PartialReasonSet::FLOW_RETURN_DEGRADED)
                .is_empty(),
            member_types_are_trustworthy: reasons
                .without(PartialReasonSet::FLOW_RETURN_UNINFERRED)
                .is_empty(),
        }
    }
}

impl ProjectSemanticDispatch<'_> {
    pub(super) fn build_classify_broad_runtime(
        &self,
        locator: &BroadRuntimeSubjectLocator,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let rehydrated = self.rehydrate_broad_runtime_subject(locator);
        let mut output = self.classify_broad_runtime_node_return_only_if(
            rehydrated.node,
            rehydrated.result_is_partial,
            ObservedRuntimePartial::observe(
                rehydrated.result_is_partial,
                rehydrated.partial_reasons,
            ),
            rehydrated.cache_suppress,
        );
        output.dep_signature = rehydrated.dep_signature;
        for root in rehydrated.observed_self_roots {
            if !output.observed_self_roots.contains(&root) {
                output.observed_self_roots.push(root);
            }
        }
        output
    }

    /// Explicit graph-instance classifier for anonymous/transient subjects.
    /// It never enters the durable family memo and is always ReturnOnly; only
    /// the canonical locator-backed key may warm.
    #[cfg(test)]
    pub(crate) fn classify_broad_runtime_transient(
        &self,
        subject: SemanticNodeId,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            self.fold_local_partial_completeness(reasons);
        }
        self.classify_broad_runtime_node_return_only_if(
            subject,
            initial_trip.is_some(),
            ObservedRuntimePartial::observe(
                initial_trip.is_some(),
                initial_trip.unwrap_or(PartialReasonSet::empty()),
            ),
            true,
        )
    }

    /// Classify `subject` into broad runtime kinds.
    ///
    /// `initial_partial` is the RESULT-level signal — it decides warm
    /// admission and never anything else. `observed` is the CLASSIFICATION
    /// -level signal: a partial whose classes leave a member's own node
    /// readable still classifies that node, so an exact sibling of an
    /// unmodelled position keeps its real constructor instead of collapsing
    /// with it.
    fn classify_broad_runtime_node_return_only_if(
        &self,
        subject: SemanticNodeId,
        initial_partial: bool,
        observed: ObservedRuntimePartial,
        force_return_only: bool,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let mut work = vec![RuntimeWork {
            node: subject,
            filter_unknown: false,
        }];
        let mut visited = FxHashSet::default();
        let mut observed_nodes = Vec::new();
        let mut kinds = Vec::new();
        let mut result_is_partial = initial_partial;
        let mut undecidable = !observed.member_types_are_trustworthy;
        let transit =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

        while let Some(item) = work.pop() {
            if undecidable {
                break;
            }
            if let Err(reasons) = self.charge_connected_work() {
                self.fold_local_partial_completeness(reasons);
                undecidable = true;
                break;
            }
            if !visited.insert(item) {
                continue;
            }
            observed_nodes.push(item.node);
            let Some(data) = self.graph().node_data(item.node) else {
                self.fold_local_partial_completeness(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA);
                undecidable = true;
                break;
            };

            let push_unknown = |kinds: &mut Vec<BroadRuntimeKind>| {
                if !item.filter_unknown {
                    kinds.push(BroadRuntimeKind::Unknown);
                }
            };

            match data.as_ref() {
                SemanticNodeData::Alias(inner) => work.push(RuntimeWork {
                    node: *inner,
                    filter_unknown: item.filter_unknown,
                }),
                SemanticNodeData::Union(arms) => {
                    for arm in arms.iter().rev() {
                        work.push(RuntimeWork {
                            node: *arm,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::Intersection(arms) => {
                    for arm in arms.iter().rev() {
                        work.push(RuntimeWork {
                            node: *arm,
                            filter_unknown: true,
                        });
                    }
                }
                SemanticNodeData::Primitive(kind) => match kind {
                    PrimitiveKind::String => kinds.push(BroadRuntimeKind::String),
                    PrimitiveKind::Number => kinds.push(BroadRuntimeKind::Number),
                    PrimitiveKind::Boolean => kinds.push(BroadRuntimeKind::Boolean),
                    PrimitiveKind::Symbol => kinds.push(BroadRuntimeKind::Symbol),
                    PrimitiveKind::Null => kinds.push(BroadRuntimeKind::Null),
                    PrimitiveKind::Undefined => push_unknown(&mut kinds),
                    PrimitiveKind::Object => kinds.push(BroadRuntimeKind::Object),
                    PrimitiveKind::BigInt
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown
                    | PrimitiveKind::Void
                    | PrimitiveKind::Never => push_unknown(&mut kinds),
                },
                SemanticNodeData::Literal(literal) => match literal {
                    LiteralValue::String(_) => kinds.push(BroadRuntimeKind::String),
                    LiteralValue::Number(_) => kinds.push(BroadRuntimeKind::Number),
                    LiteralValue::Boolean(_) => kinds.push(BroadRuntimeKind::Boolean),
                    LiteralValue::BigInt(_) => kinds.push(BroadRuntimeKind::Number),
                },
                SemanticNodeData::TemplateLiteral { .. } => kinds.push(BroadRuntimeKind::String),
                SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. } => {
                    kinds.push(BroadRuntimeKind::Array)
                }
                SemanticNodeData::Signature { .. } => kinds.push(BroadRuntimeKind::Function),
                SemanticNodeData::Object(surface) => {
                    let callable = !surface.call_signatures.is_empty()
                        || !surface.construct_signatures.is_empty();
                    if callable {
                        kinds.push(BroadRuntimeKind::Function);
                    }
                    if !callable
                        || !surface.positive_members().is_empty()
                        || !surface.index_signatures.is_empty()
                        || surface.keyspace.is_some()
                        || surface.has_known_index_signature()
                    {
                        kinds.push(BroadRuntimeKind::Object);
                    }
                }
                SemanticNodeData::ObjectSpreadProgram(_) => {
                    kinds.push(BroadRuntimeKind::Object);
                }
                SemanticNodeData::MergedDecl { .. } => kinds.push(BroadRuntimeKind::Object),
                SemanticNodeData::DeclRef { identity } => {
                    if let Some(kind) = self.runtime_nominal_identity(identity) {
                        kinds.push(broad_kind_for_nominal(kind));
                        continue;
                    }
                    undecidable = !self.push_instantiated_runtime_subject(
                        identity,
                        Arc::from([]),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::InstantiationRef { base, args } => {
                    if let Some(kind) = self.runtime_nominal_identity(base) {
                        kinds.push(broad_kind_for_nominal(kind));
                        continue;
                    }
                    undecidable = !self.push_instantiated_runtime_subject(
                        base,
                        Arc::clone(args),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    ..
                }) => {
                    let identity = crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: Default::default(),
                        decl_name: Arc::clone(name),
                    };
                    undecidable = !self.push_instantiated_runtime_subject(
                        &identity,
                        Arc::from([]),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::BareRef(_)
                | SemanticNodeData::ImportType(_)
                | SemanticNodeData::TypeOf(_) => {
                    let (resolved, nested_partial) = self.capture_runtime_node_resolution(|| {
                        self.resolve_carrier_subject_node(item.node, transit)
                    });
                    if nested_partial {
                        undecidable = true;
                    } else if resolved == item.node {
                        push_unknown(&mut kinds);
                    } else {
                        work.push(RuntimeWork {
                            node: resolved,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                        base: *object,
                        index: index.clone(),
                        mode: ProjectionMode::Navigate,
                    });
                    undecidable = !self.push_runtime_query_read(read, item, &mut work, &mut kinds);
                }
                SemanticNodeData::Conditional { .. } | SemanticNodeData::KeyOf { .. } => {
                    let (reduced, nested_partial) = self.capture_runtime_node_resolution(|| {
                        self.evaluate_deferred_semantic_node_with_context(item.node, transit)
                            .into_active_query_build_node(self)
                    });
                    if nested_partial {
                        undecidable = true;
                    } else if reduced == item.node {
                        push_unknown(&mut kinds);
                    } else {
                        work.push(RuntimeWork {
                            node: reduced,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::Mapped { .. }
                | SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::InferRef { .. }
                | SemanticNodeData::RawFallback { .. }
                | SemanticNodeData::SyntheticBinding { .. } => push_unknown(&mut kinds),
                SemanticNodeData::Opaque(error) => {
                    if let Some(reasons) = runtime_query_error_partial_reason(error) {
                        self.fold_local_partial_completeness(reasons);
                        undecidable = true;
                    } else {
                        push_unknown(&mut kinds);
                    }
                }
            }

            if undecidable {
                break;
            }
        }

        let observed_self_roots = self.observed_self_roots_from_nodes(observed_nodes);
        result_is_partial |= undecidable;
        if undecidable || kinds.is_empty() {
            kinds.clear();
            kinds.push(BroadRuntimeKind::Unknown);
        }
        let classification = BroadRuntimeClassification::new(kinds);
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Value(SemanticQueryValue::BroadRuntime(classification)),
            self.project_generation_signature(),
        )
            .into();
        output.observed_self_roots = observed_self_roots;
        output.result_is_partial = result_is_partial;
        if result_is_partial || force_return_only {
            output.cache_suppress = true;
        }
        output
    }

    fn rehydrate_broad_runtime_subject(
        &self,
        locator: &BroadRuntimeSubjectLocator,
    ) -> RehydratedRuntimeSubject {
        let owner = locator.owner();
        let canonical = owner.defining_canonical.as_ref();
        let mut dep_facts: Vec<_> = self
            .project_generation_signature()
            .iter()
            .cloned()
            .collect();
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Miss),
                dep_signature: Arc::from(dep_facts.into_boxed_slice()),
                observed_self_roots: Vec::new(),
                cache_suppress: true,
                result_is_partial: true,
                partial_reasons: PartialReasonSet::SEMANTIC_QUERY_FAULT,
            };
        };
        let indexed = serve.indexed;
        let owner_root = (Arc::clone(&owner.defining_canonical), indexed.whole_hash);
        let Some(macro_index) = usize::try_from(locator.macro_index()).ok() else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Other(Arc::from(
                    "broad-runtime macro index exceeds host index width",
                ))),
                dep_signature: Arc::from(dep_facts.into_boxed_slice()),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                result_is_partial: true,
                partial_reasons: PartialReasonSet::SEMANTIC_QUERY_FAULT,
            };
        };
        let Some(macro_kind) = indexed
            .script_analysis
            .as_ref()
            .and_then(|analysis| analysis.macros.get(macro_index))
            .map(|mac| mac.kind)
        else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Miss),
                dep_signature: Arc::from(dep_facts.into_boxed_slice()),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                result_is_partial: true,
                partial_reasons: PartialReasonSet::SEMANTIC_QUERY_FAULT,
            };
        };
        let Some(hot) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            self.ctx,
            canonical,
            macro_index,
        ) else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Miss),
                dep_signature: Arc::from(dep_facts.into_boxed_slice()),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                result_is_partial: true,
                partial_reasons: PartialReasonSet::SEMANTIC_QUERY_FAULT,
            };
        };

        let payload_read = self.execute_read(SemanticQueryKey::ResolveMacroPayload {
            owner: owner.clone(),
            macro_index,
            macro_kind,
            type_args: Arc::from([hot.node()]),
            context: self.macro_payload_context_for(canonical, ProjectionMode::Navigate),
        });
        crate::component_meta_audit::merge_dep_signature_into_local_fence(
            &mut dep_facts,
            &payload_read.dep_signature,
        );
        let mut cache_suppress = payload_read.cache_suppress;
        let mut result_is_partial = payload_read.result_is_partial;
        let mut partial_reasons = payload_read.partial_reason_classes();
        let mut node = match payload_read.value {
            QueryResult::Value(node) => node,
            QueryResult::Recursive(node) => {
                self.fold_local_partial_completeness(PartialReasonSet::SAME_PATH_RECURSION);
                result_is_partial = true;
                partial_reasons = partial_reasons.union(PartialReasonSet::SAME_PATH_RECURSION);
                node
            }
            QueryResult::Error(error) => {
                let reasons = runtime_query_error_partial_reason(&error)
                    .unwrap_or(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                self.fold_local_partial_completeness(reasons);
                result_is_partial = true;
                partial_reasons = partial_reasons.union(reasons);
                self.opaque(error)
            }
        };

        // The MEMBER projection is gated on the SHAPE, not on the partial
        // boolean. A flow-return degradation is a statement about a value
        // INSIDE a surface the substrate did produce, so the addressed
        // member is still selectable out of it; bailing here instead
        // handed the whole payload object to the classifier, which then
        // answered `Unknown` for EVERY member of a component one of whose
        // members happened to be unmodelled.
        if ObservedRuntimePartial::observe(result_is_partial, partial_reasons).shape_is_trustworthy
        {
            if let BroadRuntimeSubjectRoute::Member(name) = locator.route() {
                let provenance = match macro_kind {
                    verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                    | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => {
                        SurfaceProvenanceContext::MacroTypeArgOwnBody
                    }
                    _ => SurfaceProvenanceContext::Structural,
                };
                // Reuse the exact root-surface demand already issued by the
                // TypeInfo producer. `Shallow` materializes that filtered
                // Object; it does not navigate a non-empty path to its value.
                // Select the addressed member from the immutable surface only
                // after the warm/cold read has established its facts.
                let member_read = self.execute_read(SemanticQueryKey::ProjectPath {
                    base: node,
                    path: Arc::from([]),
                    context: ProjectionReductionContext::vue_runtime_object_surface(
                        ProjectionMode::Shallow,
                        provenance,
                    ),
                });
                crate::component_meta_audit::merge_dep_signature_into_local_fence(
                    &mut dep_facts,
                    &member_read.dep_signature,
                );
                cache_suppress |= member_read.cache_suppress;
                result_is_partial |= member_read.result_is_partial;
                partial_reasons = partial_reasons.union(member_read.partial_reason_classes());
                node = match member_read.value {
                    QueryResult::Value(node) => node,
                    QueryResult::Recursive(node) => {
                        self.fold_local_partial_completeness(PartialReasonSet::SAME_PATH_RECURSION);
                        result_is_partial = true;
                        partial_reasons =
                            partial_reasons.union(PartialReasonSet::SAME_PATH_RECURSION);
                        node
                    }
                    QueryResult::Error(error) => {
                        let reasons = runtime_query_error_partial_reason(&error)
                            .unwrap_or(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                        self.fold_local_partial_completeness(reasons);
                        result_is_partial = true;
                        partial_reasons = partial_reasons.union(reasons);
                        self.opaque(error)
                    }
                };
                if ObservedRuntimePartial::observe(result_is_partial, partial_reasons)
                    .shape_is_trustworthy
                {
                    match self.graph().node_data(node) {
                        None => {
                            self.fold_local_partial_completeness(
                                PartialReasonSet::MISSING_SEMANTIC_NODE_DATA,
                            );
                            result_is_partial = true;
                            partial_reasons =
                                partial_reasons.union(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA);
                            node = self.opaque(QueryError::Miss);
                        }
                        Some(data) => match data.as_ref() {
                            SemanticNodeData::Object(surface) => {
                                match surface.project_string_key(name.as_ref()) {
                                    crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
                                        node = member.value;
                                    }
                                    crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                                        self.fold_local_partial_completeness(
                                            PartialReasonSet::SEMANTIC_QUERY_FAULT,
                                        );
                                        result_is_partial = true;
                                        partial_reasons = partial_reasons
                                            .union(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                                        node =
                                            self.opaque(QueryError::UnrepresentableSurfaceMember);
                                    }
                                }
                            }
                            _ => {
                                self.fold_local_partial_completeness(
                                    PartialReasonSet::SEMANTIC_QUERY_FAULT,
                                );
                                result_is_partial = true;
                                partial_reasons =
                                    partial_reasons.union(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                                node = self.opaque(QueryError::UnrepresentableSurface);
                            }
                        },
                    }
                }
            }
        }

        let mut observed_self_roots = vec![owner_root];
        for root in self.observed_self_roots_from_nodes([hot.node(), node]) {
            if !observed_self_roots.contains(&root) {
                observed_self_roots.push(root);
            }
        }
        RehydratedRuntimeSubject {
            node,
            dep_signature: Arc::from(dep_facts.into_boxed_slice()),
            observed_self_roots,
            cache_suppress,
            result_is_partial,
            partial_reasons,
        }
    }

    fn push_instantiated_runtime_subject(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        args: Arc<[SemanticNodeId]>,
        item: RuntimeWork,
        transit: ProjectionReductionContext,
        work: &mut Vec<RuntimeWork>,
        kinds: &mut Vec<BroadRuntimeKind>,
    ) -> bool {
        let slot = self.type_slot_for(
            Arc::clone(&identity.canonical_id),
            identity.owner,
            Arc::clone(&identity.decl_name),
        );
        let read = self.execute_read(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                args,
                self.instantiate_context_for(&identity.canonical_id, transit),
            ),
        ));
        self.push_runtime_query_read(read, item, work, kinds)
    }

    fn push_runtime_query_read(
        &self,
        read: CacheRead<QueryResult<SemanticNodeId>>,
        item: RuntimeWork,
        work: &mut Vec<RuntimeWork>,
        kinds: &mut Vec<BroadRuntimeKind>,
    ) -> bool {
        let result_is_partial = read.result_is_partial;
        if result_is_partial {
            match &read.value {
                QueryResult::Recursive(_) => {
                    self.fold_local_partial_completeness(PartialReasonSet::SAME_PATH_RECURSION);
                }
                QueryResult::Error(error) => {
                    if let Some(reasons) = runtime_query_error_partial_reason(error) {
                        self.fold_local_partial_completeness(reasons);
                    }
                }
                QueryResult::Value(_) => {}
            }
            return false;
        }
        match read.value {
            QueryResult::Value(node) if node != item.node => {
                work.push(RuntimeWork {
                    node,
                    filter_unknown: item.filter_unknown,
                });
                true
            }
            QueryResult::Value(_) | QueryResult::Error(QueryError::Miss) => {
                if !item.filter_unknown {
                    kinds.push(BroadRuntimeKind::Unknown);
                }
                true
            }
            QueryResult::Recursive(_) => {
                self.fold_local_partial_completeness(PartialReasonSet::SAME_PATH_RECURSION);
                false
            }
            QueryResult::Error(error) => {
                if let Some(reasons) = runtime_query_error_partial_reason(&error) {
                    self.fold_local_partial_completeness(reasons);
                    false
                } else {
                    if !item.filter_unknown {
                        kinds.push(BroadRuntimeKind::Unknown);
                    }
                    true
                }
            }
        }
    }

    fn capture_runtime_node_resolution(
        &self,
        resolve: impl FnOnce() -> SemanticNodeId,
    ) -> (SemanticNodeId, bool) {
        let observation = super::BuildLocalTaintGuard::push(&self.build_local_taint);
        let resolved = resolve();
        let observed = observation.finish();
        self.fold_into_top_build_local_taint_with(
            observed.result_is_partial,
            observed.cache_suppress,
            observed.partial_reasons,
        );
        (resolved, observed.result_is_partial)
    }
}

fn runtime_query_error_partial_reason(error: &QueryError) -> Option<PartialReasonSet> {
    match error {
        QueryError::Miss | QueryError::DeclPlaceholder { .. } => None,
        QueryError::BudgetExceeded(_) => Some(PartialReasonSet::BUDGET_EXCEEDED),
        QueryError::Cancelled => Some(PartialReasonSet::CANCELLED),
        QueryError::UnstableState { .. } => Some(PartialReasonSet::UNSTABLE_STATE),
        QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => Some(PartialReasonSet::SAME_PATH_RECURSION),
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::Other(_)
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::RaiseMiss
        | QueryError::OpenSurface
        | QueryError::UnrepresentableSurface
        | QueryError::UnrepresentableSurfaceMember => Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        // The flow substrate's positional marker states that a value
        // EXISTS here and this substrate cannot name it — a genuine
        // partial, never the benign "not found" `Miss` takes. Its class is
        // the flow substrate's OWN, not the generic query fault: a
        // consumer that CONTAINS a positional non-modelling must see the
        // same class whether it observes the marker through the flow
        // result or by reading the marker node itself, or the second
        // observation silently un-contains the first.
        QueryError::UnmodeledPosition => Some(PartialReasonSet::FLOW_RETURN_UNINFERRED),
    }
}

fn broad_kind_for_nominal(kind: RuntimeNominal) -> BroadRuntimeKind {
    match kind {
        RuntimeNominal::Date => BroadRuntimeKind::Date,
        RuntimeNominal::Map => BroadRuntimeKind::Map,
        RuntimeNominal::Set => BroadRuntimeKind::Set,
        RuntimeNominal::WeakMap => BroadRuntimeKind::WeakMap,
        RuntimeNominal::WeakSet => BroadRuntimeKind::WeakSet,
        RuntimeNominal::Promise => BroadRuntimeKind::Promise,
        RuntimeNominal::Error => BroadRuntimeKind::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_query_error_preserves_exact_partial_reason() {
        assert_eq!(
            runtime_query_error_partial_reason(&QueryError::Cancelled),
            Some(PartialReasonSet::CANCELLED)
        );
    }

    /// The flow substrate's positional marker carries the flow substrate's
    /// OWN class, so observing it as a NODE and observing it through the
    /// flow result are the same observation.
    ///
    /// Reading it as the generic `SEMANTIC_QUERY_FAULT` makes the second
    /// observation un-contain the first: a consumer that contains
    /// `FLOW_RETURN_UNINFERRED` sees an uncontained class arrive from the
    /// very node the contained class produced, and the surface it was
    /// about to publish per-member is deleted instead. It also flips
    /// `ObservedRuntimePartial::shape_is_trustworthy` to `false`, which
    /// short-circuits the member projection this file exists to perform.
    ///
    /// The mapping is asserted directly HERE, and exercised through the
    /// production classification path by
    /// `broad_runtime_tests::the_positional_marker_classifies_with_the_
    /// flow_class_through_the_classifier`. It is not asserted at a
    /// SURFACE-level fixture because none can see the class:
    /// `runtime_props_derive_each_member_from_that_members_own_evidence`
    /// does drive a marker node into the `Opaque` arm of
    /// `classify_runtime_kinds`, but both spellings stop that walk
    /// identically (`undecidable`) and the folded class does not escape it
    /// to the macro-codegen completeness the publish/refuse decision
    /// reads.
    ///
    /// The `QueryResult::Error` route into this function
    /// (`rehydrate_broad_runtime_subject`, `push_runtime_query_read`) is
    /// live but cannot carry this variant today: the marker exists only as
    /// a NODE (`unmodeled_position_marker` interns
    /// `SemanticNodeData::Opaque`), and no producer returns
    /// `QueryResult::Error(QueryError::UnmodeledPosition)`. The arm covers
    /// it so that a future producer inherits the right class rather than
    /// the wildcard's.
    #[test]
    fn the_positional_marker_carries_the_flow_class_not_the_generic_query_fault() {
        assert_eq!(
            runtime_query_error_partial_reason(&QueryError::UnmodeledPosition),
            Some(PartialReasonSet::FLOW_RETURN_UNINFERRED),
        );
        assert!(
            ObservedRuntimePartial::observe(true, PartialReasonSet::FLOW_RETURN_UNINFERRED)
                .shape_is_trustworthy,
            "the addressed member is still selectable out of a surface whose one \
             unmodelled position says so"
        );
        // CONTROL — a genuine query fault says nothing survived, and must
        // still short-circuit both the member projection and the
        // classification.
        assert_eq!(
            runtime_query_error_partial_reason(&QueryError::UnrepresentableSurface),
            Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        );
        let faulted = ObservedRuntimePartial::observe(true, PartialReasonSet::SEMANTIC_QUERY_FAULT);
        assert!(!faulted.shape_is_trustworthy);
        assert!(!faulted.member_types_are_trustworthy);
        // CONTROL — the frame-wide class keeps the SHAPE but not the
        // member types.
        let unverified =
            ObservedRuntimePartial::observe(true, PartialReasonSet::FLOW_RETURN_UNVERIFIED);
        assert!(unverified.shape_is_trustworthy);
        assert!(!unverified.member_types_are_trustworthy);
        // CONTROL — an UNNAMED partial fails CLOSED. The empty set is
        // vacuously a subset of every class, so a structural read alone
        // would make the least-informative partial the most trusted one.
        let unnamed = ObservedRuntimePartial::observe(true, PartialReasonSet::empty());
        assert!(!unnamed.shape_is_trustworthy);
        assert!(!unnamed.member_types_are_trustworthy);
        // CONTROL — a MIXED observation is governed by its worst class.
        let mixed = ObservedRuntimePartial::observe(
            true,
            PartialReasonSet::FLOW_RETURN_UNINFERRED.union(PartialReasonSet::BUDGET_EXCEEDED),
        );
        assert!(!mixed.shape_is_trustworthy);
        assert!(!mixed.member_types_are_trustworthy);
    }
}
