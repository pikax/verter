use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::intrinsic_registry::RuntimeNominal;
use crate::locator_identity::{BroadRuntimeSubjectLocator, BroadRuntimeSubjectRoute};
use crate::semantic_query::{
    BroadRuntimeClassification, BroadRuntimeKind, CacheRead, DepSignature, FactStatus,
    LiteralValue, PartialReasonSet, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, SurfaceProvenanceContext,
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
    fact_status: FactStatus,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    pub(super) fn build_classify_broad_runtime(
        &self,
        locator: &BroadRuntimeSubjectLocator,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let rehydrated = self.rehydrate_broad_runtime_subject(locator);
        let mut output = self.classify_broad_runtime_node_return_only_if(
            rehydrated.node,
            rehydrated.fact_status,
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn classify_broad_runtime_transient(
        &self,
        subject: SemanticNodeId,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let frame = super::BuildLocalTaintGuard::push(&self.build_local_taint);
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            self.fold_local_partial_completeness(reasons);
        }
        let output = self.classify_broad_runtime_node_return_only_if(
            subject,
            initial_trip.map_or(FactStatus::Complete, FactStatus::unavailable),
            true,
        );
        let observed = frame.finish();
        self.fold_observed_frame_into_top(&observed);
        output
    }

    /// Classify `subject` into broad runtime kinds.
    ///
    /// Classification consumes only an exact subject fact. A selected member
    /// can supply that fact independently of its incomplete whole payload.
    fn classify_broad_runtime_node_return_only_if(
        &self,
        subject: SemanticNodeId,
        subject_status: FactStatus,
        force_return_only: bool,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let mut work = vec![RuntimeWork {
            node: subject,
            filter_unknown: false,
        }];
        let mut visited = FxHashSet::default();
        let mut observed_nodes = Vec::new();
        let mut kinds = Vec::new();
        let mut result_is_partial = !subject_status.is_complete();
        let mut undecidable = !subject_status.is_complete();
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
                // The operation is not itself a runtime shape; its operands
                // are, so they are the work — never a blanket Unknown.
                SemanticNodeData::IntrinsicApplication { args, .. } => {
                    for arg in args.iter().rev() {
                        work.push(RuntimeWork {
                            node: *arg,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::Alias(inner) => work.push(RuntimeWork {
                    node: *inner,
                    filter_unknown: item.filter_unknown,
                }),
                SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                    work.push(RuntimeWork {
                        node: *surface,
                        filter_unknown: item.filter_unknown,
                    })
                }
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
                // An enum member's runtime value is its base value.
                SemanticNodeData::EnumLiteral(literal) => work.push(RuntimeWork {
                    node: literal.base,
                    filter_unknown: item.filter_unknown,
                }),
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
                SemanticNodeData::Signature { .. } | SemanticNodeData::DeferredCallable(_) => {
                    kinds.push(BroadRuntimeKind::Function)
                }
                SemanticNodeData::Object(surface) => {
                    // Callability is a signature-discovery answer, never the
                    // surface's own lists: a surface merged for presentation
                    // carries lists the kernel did not produce.
                    let (call, construct) = match self.shared_signature_buckets(item.node) {
                        Ok(buckets) => buckets,
                        Err(_) => {
                            undecidable = true;
                            continue;
                        }
                    };
                    let callable = !call.is_empty() || !construct.is_empty();
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
                        Arc::clone(args.as_arc()),
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
                        canonical_id: canonical_id.clone(),
                        owner: *owner,
                        whole_hash: Default::default(),
                        decl_name: name.clone(),
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
                | SemanticNodeData::TypeOf(_)
                | SemanticNodeData::TypeOfNominal(_) => {
                    // The runtime constructor is a STRUCTURAL question: a
                    // `unique symbol`-typed prop emits `Symbol`, exactly as
                    // its widened inhabitant does. A nominal carrier is
                    // terminal under carrier resolution, so the
                    // `resolved == item.node` rail below would read it as an
                    // unresolved reference and collapse the WHOLE
                    // constructor list to empty (`Unknown` with no
                    // Boolean/Function present clears `constructors`), not
                    // merely this one entry. Widen first; the deferred
                    // shell falls through to its ordinary resolution.
                    if let Some(widened) = self.widened_nominal_typeof(item.node) {
                        work.push(RuntimeWork {
                            node: widened,
                            filter_unknown: item.filter_unknown,
                        });
                        continue;
                    }
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
        let classification = if undecidable {
            let reasons = self
                .build_local_taint
                .borrow()
                .last()
                .map(|frame| frame.partial_reasons)
                .unwrap_or_default();
            BroadRuntimeClassification::unavailable(
                crate::semantic_query::surface_resolution::NonEmptyReasons::new(
                    subject_status.completeness().reasons(),
                )
                .or_else(|| {
                    crate::semantic_query::surface_resolution::NonEmptyReasons::new(reasons)
                })
                .unwrap_or_else(|| {
                    crate::semantic_query::surface_resolution::NonEmptyReasons::of(
                        crate::semantic_query::PartialReason::SemanticQueryFault,
                    )
                }),
            )
        } else {
            BroadRuntimeClassification::new(kinds)
        };
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Value(SemanticQueryValue::BroadRuntime(classification)),
            self.project_generation_signature(),
        )
            .into();
        output.observed_self_roots = observed_self_roots;
        output.fold_partial(result_is_partial);
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
                dep_signature: crate::semantic_query::DepSignature::new(dep_facts),
                observed_self_roots: Vec::new(),
                cache_suppress: true,
                fact_status: FactStatus::unavailable(PartialReasonSet::SEMANTIC_QUERY_FAULT),
            };
        };
        let indexed = serve.indexed;
        let owner_root = (owner.defining_canonical.clone(), indexed.whole_hash);
        let Some(macro_index) = usize::try_from(locator.macro_index()).ok() else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Other(verter_type_expr::OwnedStr::from(
                    "broad-runtime macro index exceeds host index width",
                ))),
                dep_signature: crate::semantic_query::DepSignature::new(dep_facts),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                fact_status: FactStatus::unavailable(PartialReasonSet::SEMANTIC_QUERY_FAULT),
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
                dep_signature: crate::semantic_query::DepSignature::new(dep_facts),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                fact_status: FactStatus::unavailable(PartialReasonSet::SEMANTIC_QUERY_FAULT),
            };
        };
        let Some(product) = self.macro_type_arg_hot_ref(canonical, macro_index) else {
            self.fold_local_partial_completeness(PartialReasonSet::SEMANTIC_QUERY_FAULT);
            return RehydratedRuntimeSubject {
                node: self.opaque(QueryError::Miss),
                dep_signature: crate::semantic_query::DepSignature::new(dep_facts),
                observed_self_roots: vec![owner_root],
                cache_suppress: true,
                fact_status: FactStatus::unavailable(PartialReasonSet::SEMANTIC_QUERY_FAULT),
            };
        };

        let payload_observation = self.observe_read(|| {
            self.execute_read(SemanticQueryKey::ResolveMacroPayload {
                owner: owner.clone(),
                macro_index,
                macro_kind,
                type_args: Arc::from([product.hot.node()]),
                context: self.macro_payload_context_for(canonical, ProjectionMode::Navigate),
            })
        });
        let payload_read = payload_observation.value();
        let payload_complete = payload_read.fact_status.is_complete();
        let mut independent_value = false;
        let mut cache_suppress = false;
        let mut fact_status = payload_read.fact_status;
        let mut node = match &payload_read.value {
            QueryResult::Value(node) => *node,
            QueryResult::Recursive(node) => {
                fact_status = fact_status.join(FactStatus::unavailable(
                    PartialReasonSet::SAME_PATH_RECURSION,
                ));
                *node
            }
            QueryResult::Error(error) => {
                let reasons = runtime_query_error_partial_reason(error)
                    .unwrap_or(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                fact_status = fact_status.join(FactStatus::unavailable(reasons));
                self.opaque(error.clone())
            }
        };

        // Demand the original source independently. An incomplete whole
        // payload cannot certify member values, and its causes cannot prevent
        // an independently Complete member fact from being adopted.
        {
            if let BroadRuntimeSubjectRoute::Member(name) = locator.route() {
                let provenance = match macro_kind {
                    verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps
                    | verter_session_query::analysis::types::AnalyzedMacroKind::WithDefaults => {
                        SurfaceProvenanceContext::MacroTypeArgOwnBody
                    }
                    _ => SurfaceProvenanceContext::Structural,
                };
                // Demand this member through the shared path producer. Its
                // independent flow-member proof can establish an exact value
                // beside a sibling gap; selecting from a presence-only root
                // surface cannot establish that value's classification.
                // An exact whole surface already supplies exact member values,
                // including their authored union order. A partial surface does
                // not: discard that probe and demand the selected member.
                let probe = self.observe_read(|| {
                    self.execute_read(SemanticQueryKey::ProjectPath {
                        base: node,
                        path: Arc::from([]),
                        context: ProjectionReductionContext::vue_runtime_object_surface(
                            ProjectionMode::Shallow,
                            provenance,
                        ),
                    })
                });
                let selected = if payload_complete && probe.value().fact_status.is_complete() {
                    match probe.value().value {
                        QueryResult::Value(surface) => {
                            self.graph()
                                .node_data(surface)
                                .and_then(|data| match data.as_ref() {
                                    SemanticNodeData::Object(view) => view
                                        .positive_members()
                                        .iter()
                                        .find(|member| {
                                            member.key.as_known().is_some_and(|key| {
                                                key.as_string() == Some(name.as_ref())
                                            })
                                        })
                                        .map(|member| member.value),
                                    _ => None,
                                })
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(selected) = selected {
                    if let Ok(read) = probe.adopt(self) {
                        crate::fact_signature_helpers::merge_dep_signature_into_local_fence(
                            &mut dep_facts,
                            &read.dep_signature,
                        );
                        cache_suppress |= read.cache_suppress;
                        fact_status = FactStatus::Complete;
                        node = selected;
                    }
                } else {
                    let _ = probe.discard();
                    let member_read = self.execute_read(SemanticQueryKey::ProjectPath {
                        base: product.hot.node(),
                        path: Arc::from([crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(name.as_ref()),
                        )]),
                        context: ProjectionReductionContext::vue_runtime_object_surface(
                            ProjectionMode::Navigate,
                            provenance,
                        ),
                    });
                    crate::fact_signature_helpers::merge_dep_signature_into_local_fence(
                        &mut dep_facts,
                        &member_read.dep_signature,
                    );
                    cache_suppress |= member_read.cache_suppress;
                    if member_read.fact_status.is_complete() {
                        independent_value = true;
                        fact_status = FactStatus::Complete;
                    } else {
                        fact_status = fact_status.join(member_read.fact_status);
                    }
                    node = match member_read.value {
                        QueryResult::Value(node) => node,
                        QueryResult::Recursive(node) => {
                            self.fold_local_partial_completeness(
                                PartialReasonSet::SAME_PATH_RECURSION,
                            );
                            fact_status = fact_status.join(FactStatus::unavailable(
                                PartialReasonSet::SAME_PATH_RECURSION,
                            ));
                            node
                        }
                        QueryResult::Error(error) => {
                            let reasons = runtime_query_error_partial_reason(&error)
                                .unwrap_or(PartialReasonSet::SEMANTIC_QUERY_FAULT);
                            self.fold_local_partial_completeness(reasons);
                            fact_status = fact_status.join(FactStatus::unavailable(reasons));
                            self.opaque(error)
                        }
                    };
                }
            }
        }

        // The original payload's observations are declined only when the
        // original source independently certified the selected member value.
        // Execution aborts propagate through either disposition.
        let payload_validity = if independent_value {
            payload_observation.discard()
        } else {
            crate::fact_signature_helpers::merge_dep_signature_into_local_fence(
                &mut dep_facts,
                &payload_read.dep_signature,
            );
            cache_suppress |= payload_read.cache_suppress;
            payload_observation.adopt(self)
        };
        if let Err(abort) = payload_validity {
            node = self.opaque(QueryError::ExecutionAborted(abort));
            fact_status = FactStatus::unavailable(abort.reason_class());
            cache_suppress = true;
        }

        let mut observed_self_roots = vec![owner_root];
        for root in self.observed_self_roots_from_nodes([product.hot.node(), node]) {
            if !observed_self_roots.contains(&root) {
                observed_self_roots.push(root);
            }
        }
        RehydratedRuntimeSubject {
            node,
            dep_signature: crate::semantic_query::DepSignature::new(dep_facts),
            observed_self_roots,
            cache_suppress,
            fact_status,
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
        let result_is_partial = read.result_is_partial();
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
        self.fold_observed_frame_into_top(&observed);
        (resolved, observed.result_is_partial)
    }
}

fn runtime_query_error_partial_reason(error: &QueryError) -> Option<PartialReasonSet> {
    match error {
        QueryError::ExecutionAborted(abort) => Some(abort.reason_class()),
        // The checker's recovered error type is a complete answer.
        QueryError::Miss
        | QueryError::DeclPlaceholder { .. }
        | QueryError::CheckerRecovery {
            basis: crate::semantic_query::RecoveryBasis::Certified,
            ..
        } => None,
        QueryError::CheckerRecovery {
            basis: crate::semantic_query::RecoveryBasis::Budget,
            ..
        } => Some(PartialReasonSet::OPERATION_BUDGET),
        QueryError::BudgetExceeded(_) => Some(PartialReasonSet::BUDGET_EXCEEDED),
        QueryError::Cancelled => Some(PartialReasonSet::CANCELLED),
        QueryError::UnstableState { .. } | QueryError::StaleSemanticOperand => {
            Some(PartialReasonSet::UNSTABLE_STATE)
        }
        QueryError::IncompleteSemanticOperand { reasons } => Some(*reasons),
        QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => Some(PartialReasonSet::SAME_PATH_RECURSION),
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::ForeignSemanticOperand
        | QueryError::Other(_)
        | QueryError::PermissiveWildcard
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
        RuntimeNominal::Function => BroadRuntimeKind::Function,
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

    /// Diagnostic causes preserve the producer's positional class. These
    /// classes explain refusal; the adopted fact alone authorizes a runtime
    /// classification or independent member value.
    #[test]
    fn the_positional_marker_carries_the_flow_class_not_the_generic_query_fault() {
        assert_eq!(
            runtime_query_error_partial_reason(&QueryError::UnmodeledPosition),
            Some(PartialReasonSet::FLOW_RETURN_UNINFERRED),
        );
        assert_eq!(
            runtime_query_error_partial_reason(&QueryError::UnrepresentableSurface),
            Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        );
    }
}
