//! Runtime-bundle projection, broad-runtime classification, and the shared
//! macro ordinal / anchor / failure helpers for the Vue macro codegen producer.
//!
//! Sibling half of the parent module `impl VerterHost` orchestrator; see the
//! parent module docs for the overall producer contract.

use super::*;
use verter_type_engine::semantic_query::{root_narrowing_fact, FactResult};

/// Prove a row-local missing dependency without conflating it with an
/// authored `unknown` or with a reference nested below a constructor-bearing
/// shell. The semantic analyzer is the authority for which imported heads are
/// MEMBER-tier dependencies; this walk only follows the transparent shapes
/// that participate in broad constructor inference.
pub(super) fn direct_member_dependency_is_missing(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    subject: verter_type_engine::semantic_query::SemanticNodeId,
    dependency_names: &FxHashSet<&str>,
) -> bool {
    if dependency_names.is_empty() {
        return false;
    }

    let carrier_context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    let eager_context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Expanded);
    let mut work = vec![(subject, false)];
    let mut visited = FxHashSet::default();

    while let Some((node, tracked_dependency)) = work.pop() {
        if !visited.insert((node, tracked_dependency)) {
            continue;
        }
        let Some(data) = dispatch.graph().read_in_scope(node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => work.push((*inner, tracked_dependency)),
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let arms = composite.composite_members().expect("composite arm");
                work.extend(arms.iter().copied().map(|arm| (arm, tracked_dependency)));
            }
            SemanticNodeData::BareRef(_) => {
                let (name, _) = data.bare_ref_head().expect("BareRef carrier head");
                let tracked_dependency = tracked_dependency || dependency_names.contains(name);
                drop(data);
                let Some(resolved) = resolve_member_carrier(
                    dispatch,
                    node,
                    tracked_dependency,
                    eager_context,
                    carrier_context,
                ) else {
                    return false;
                };
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_) => {
                let (root, _) = data.typeof_head().expect("TypeOf carrier head");
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(root.name.as_ref());
                drop(data);
                let Some(resolved) = resolve_member_carrier(
                    dispatch,
                    node,
                    tracked_dependency,
                    eager_context,
                    carrier_context,
                ) else {
                    return false;
                };
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::ImportType(_) => {
                drop(data);
                let Some(resolved) = resolve_member_carrier(
                    dispatch,
                    node,
                    tracked_dependency,
                    eager_context,
                    carrier_context,
                ) else {
                    return false;
                };
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::DeclRef { identity } => {
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(identity.decl_name.as_ref());
                let identity = identity.clone();
                drop(data);
                let (resolved, local) = observe_local(|| {
                    dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: identity.canonical_id.clone(),
                            owner: identity.owner,
                            local_scope: None,
                            binder_scope_id:
                                verter_type_engine::semantic_query::BinderScopeId::file_scope(
                                    identity.owner,
                                ),
                        },
                        name: identity.decl_name.clone(),
                    }))
                });
                match resolved {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency && !local.is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(base.decl_name.as_ref());
                let base = base.clone();
                let args: Arc<[_]> = Arc::from(&args[..]);
                drop(data);
                let (resolved, local) = observe_local(|| {
                    dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                        verter_type_engine::semantic_query::InstantiateKey::new(
                            dispatch.type_slot_for(
                                base.canonical_id.clone(),
                                base.owner,
                                base.decl_name.clone(),
                            ),
                            args,
                            dispatch.instantiate_context_for(
                                &base.canonical_id,
                                if tracked_dependency {
                                    eager_context
                                } else {
                                    carrier_context
                                },
                            ),
                        ),
                    ))
                });
                match resolved {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency && !local.is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::Opaque(
                verter_type_engine::semantic_query::QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    ..
                },
            ) => {
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(name.as_ref());
                let canonical_id = canonical_id.clone();
                let owner = *owner;
                let name = name.clone();
                drop(data);
                let (resolved, local) = observe_local(|| {
                    dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                        verter_type_engine::semantic_query::InstantiateKey::new(
                            dispatch.type_slot_for(canonical_id.clone(), owner, name),
                            Arc::from([]),
                            dispatch.instantiate_context_for(
                                &canonical_id,
                                if tracked_dependency {
                                    eager_context
                                } else {
                                    carrier_context
                                },
                            ),
                        ),
                    ))
                });
                match resolved {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency && !local.is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::Opaque(verter_type_engine::semantic_query::QueryError::Miss)
                if tracked_dependency =>
            {
                return true;
            }
            // Constructor-bearing shells are terminals. In particular, never
            // descend into Object/Array/Tuple children: those references are
            // nested and intentionally absent from `macro_type_deps`.
            _ => {}
        }
    }

    false
}

fn is_definitely_non_object_root(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    mut subject: verter_type_engine::semantic_query::SemanticNodeId,
) -> bool {
    let mut visited = FxHashSet::default();
    let resolution_context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    while visited.insert(subject) {
        let Some(data) = dispatch.graph().read_in_scope(subject) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Signature { .. } => return true,
            SemanticNodeData::Alias(inner) => subject = *inner,
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_) => {
                drop(data);
                // A nominal (`unique symbol`) carrier is terminal under
                // carrier resolution, so the `resolved == subject` rail
                // below would read it as an unresolved reference. It IS a
                // definitely-non-object root — exactly as its widened
                // `symbol` inhabitant is — so widen and let the primitive
                // arm decide.
                if let Some(widened) = dispatch.widened_nominal_typeof(subject) {
                    subject = widened;
                    continue;
                }
                let resolved = dispatch.resolve_carrier_subject_node(subject, resolution_context);
                if resolved == subject {
                    return false;
                }
                subject = resolved;
            }
            SemanticNodeData::DeclRef { identity } => {
                let identity = identity.clone();
                drop(data);
                let resolved =
                    dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: identity.canonical_id.clone(),
                            owner: identity.owner,
                            local_scope: None,
                            binder_scope_id:
                                verter_type_engine::semantic_query::BinderScopeId::file_scope(
                                    identity.owner,
                                ),
                        },
                        name: identity.decl_name.clone(),
                    }));
                let QueryResult::Value(resolved) = resolved else {
                    return false;
                };
                if resolved.value == subject {
                    return false;
                }
                subject = resolved.value;
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let base = base.clone();
                let args: Arc<[_]> = Arc::from(&args[..]);
                drop(data);
                let resolved = dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    verter_type_engine::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(
                            base.canonical_id.clone(),
                            base.owner,
                            base.decl_name.clone(),
                        ),
                        args,
                        dispatch.instantiate_context_for(&base.canonical_id, resolution_context),
                    ),
                ));
                let QueryResult::Value(resolved) = resolved else {
                    return false;
                };
                if resolved.value == subject {
                    return false;
                }
                subject = resolved.value;
            }
            SemanticNodeData::Opaque(
                verter_type_engine::semantic_query::QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    ..
                },
            ) => {
                let canonical_id = canonical_id.clone();
                let owner = *owner;
                let name = name.clone();
                drop(data);
                let resolved = dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    verter_type_engine::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(canonical_id.clone(), owner, name),
                        Arc::from([]),
                        dispatch.instantiate_context_for(&canonical_id, resolution_context),
                    ),
                ));
                let QueryResult::Value(resolved) = resolved else {
                    return false;
                };
                if resolved.value == subject {
                    return false;
                }
                subject = resolved.value;
            }
            _ => return false,
        }
    }
    false
}

/// Run the conservative non-object predicate in an exploratory completeness
/// scope.
///
/// A `true` result is the root-narrowing fact: the walk's own coverage is
/// complete and the terminal is non-object. An incomplete walk is not that
/// claim. Its partiality is discarded here; the following surface demand
/// re-observes it and does not inherit a definitive negative.
pub(super) fn probe_definitely_non_object_root(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    subject: verter_type_engine::semantic_query::SemanticNodeId,
) -> bool {
    let probe_scope = verter_type_engine::request_context::ColdComputeCompletenessScope::enter();
    let definitely_non_object = is_definitely_non_object_root(dispatch, subject);
    let observed = verter_type_engine::request_context::current_cold_compute_completeness();
    probe_scope.discard();
    if !definitely_non_object {
        return false;
    }
    let domain = demand_fact(observed).status();
    root_narrowing_fact(&domain, &[]).exact().is_some()
}

pub(super) struct RuntimeClassification {
    pub(super) constructors: OrderedRuntimeConstructors,
    pub(super) skip_check: bool,
}

pub(super) fn classify_runtime(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    subject: BroadRuntimeSubjectLocator,
    counters: &mut VueMacroCodegenCounters,
) -> Result<RuntimeClassification, ProjectionFailure> {
    counters.runtime_classifier_calls += 1;
    let result = dispatch.execute_fact(dispatch.broad_runtime_key_for(subject));
    let classification = match result {
        Ok(verter_type_engine::semantic_query::FactResult::Complete(
            SemanticQueryValue::BroadRuntime(classification),
        )) => classification,
        Ok(verter_type_engine::semantic_query::FactResult::Complete(_)) => {
            return Err(ProjectionFailure::Unsupported(
                UnsupportedReason::SemanticConstruct,
            ));
        }
        Ok(
            verter_type_engine::semantic_query::FactResult::Approximate { causes, .. }
            | verter_type_engine::semantic_query::FactResult::Unavailable { causes },
        ) => {
            return Err(failure_from_causes(causes.get()));
        }
        Err(_) => {
            return Err(ProjectionFailure::Partial(
                MacroPartialReason::IncompleteTraversal,
            ))
        }
    };

    let skip_check = classification.kinds().contains(&BroadRuntimeKind::Unknown)
        && classification
            .kinds()
            .iter()
            .any(|kind| matches!(kind, BroadRuntimeKind::Boolean | BroadRuntimeKind::Function));
    let constructors = if classification.kinds().contains(&BroadRuntimeKind::Unknown) && !skip_check
    {
        OrderedRuntimeConstructors::default()
    } else {
        OrderedRuntimeConstructors::from_ordered(
            classification
                .kinds()
                .iter()
                .copied()
                .map(runtime_constructor),
        )
    };
    Ok(RuntimeClassification {
        constructors,
        skip_check,
    })
}

fn runtime_constructor(kind: BroadRuntimeKind) -> RuntimeConstructor {
    match kind {
        BroadRuntimeKind::String => RuntimeConstructor::String,
        BroadRuntimeKind::Number => RuntimeConstructor::Number,
        BroadRuntimeKind::Boolean => RuntimeConstructor::Boolean,
        BroadRuntimeKind::Symbol => RuntimeConstructor::Symbol,
        BroadRuntimeKind::Null => RuntimeConstructor::Null,
        BroadRuntimeKind::Array => RuntimeConstructor::Array,
        BroadRuntimeKind::Function => RuntimeConstructor::Function,
        BroadRuntimeKind::Date => RuntimeConstructor::Date,
        BroadRuntimeKind::Map => RuntimeConstructor::Map,
        BroadRuntimeKind::Set => RuntimeConstructor::Set,
        BroadRuntimeKind::WeakMap => RuntimeConstructor::WeakMap,
        BroadRuntimeKind::WeakSet => RuntimeConstructor::WeakSet,
        BroadRuntimeKind::Promise => RuntimeConstructor::Promise,
        BroadRuntimeKind::Error => RuntimeConstructor::Error,
        BroadRuntimeKind::Object => RuntimeConstructor::Object,
        BroadRuntimeKind::Unknown => RuntimeConstructor::Unknown,
    }
}

/// Decide only resolved, concrete member shapes. Open/opaque member payloads
/// retain the existing conservative emit-name projection, while concrete
/// scalar/object/array payloads cannot satisfy Vue's tuple/function payload
/// contract and invalidate the enclosing macro.
pub(super) fn emits_surface_has_invalid_member(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    surface: &TypeInfoSurface,
    context: ProjectionReductionContext,
) -> bool {
    use verter_type_engine::semantic_query::{PrimitiveKind, SemanticNodeData};

    let shape_context = ProjectionReductionContext::published(ProjectionMode::Navigate)
        .with_orthogonal_axes_from(context);
    surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
        .any(|member| {
            let Some(node) = dispatch
                .normalize_node_for_structural_fact_demand(member.value, shape_context)
                .into_complete_node()
            else {
                return false;
            };
            match dispatch.graph().read_in_scope(node).as_deref() {
                Some(SemanticNodeData::Primitive(kind)) => {
                    !matches!(kind, PrimitiveKind::Any | PrimitiveKind::Unknown)
                }
                Some(
                    SemanticNodeData::Literal(_)
                    | SemanticNodeData::Object(_)
                    | SemanticNodeData::Array { .. }
                    | SemanticNodeData::TemplateLiteral { .. },
                ) => true,
                _ => false,
            }
        })
}

pub(super) fn authored_emit_order(anchor: MacroAnchor) -> (u8, u32) {
    match anchor {
        MacroAnchor::Authored { member_ordinal, .. } => (0, member_ordinal.get()),
        MacroAnchor::MacroArgument { .. } | MacroAnchor::Synthesized { .. } => (1, 0),
    }
}

/// Anchor for one `defineProps` member of the TSC splice lane — the
/// runtime lane's twin policy now lives in the semantic kernel
/// (`type_info::non_flow::prop_member_anchor`); the TSC authored-splice
/// lane keeps this copy until C3 closes it.
pub(super) fn member_anchor(mac: &AnalyzedMacro, payload_index: usize, name: &str) -> MacroAnchor {
    let Some(ordinal) = mac
        .prop_fields
        .iter()
        .filter(|field| span_is_owned_by_macro(field.span, mac.span))
        .position(|field| field.name == name)
    else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(payload_index),
        member_ordinal: AuthoredMemberOrdinal::new(member_ordinal(ordinal)),
    }
}

pub(super) fn authored_emit_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    name: &str,
) -> MacroAnchor {
    let Some(ordinal) = mac
        .emit_fields
        .iter()
        .filter(|field| span_is_owned_by_macro(field.span, mac.span))
        .position(|field| field.name == name)
    else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(effective_index),
        member_ordinal: AuthoredMemberOrdinal::new(member_ordinal(ordinal)),
    }
}

fn span_is_owned_by_macro(member: verter_span::Span, mac: verter_span::Span) -> bool {
    member.start >= mac.start && member.end <= mac.end
}

/// Run `body` in a nested completeness scope and return what that body
/// itself observed. Dropping the scope bubbles those reasons into the
/// parent; the returned completeness is the body's own, read before the drop.
pub(super) fn observe_local<R>(body: impl FnOnce() -> R) -> (R, ResultCompleteness) {
    let scope = verter_type_engine::request_context::ColdComputeCompletenessScope::enter();
    let value = body();
    let local = verter_type_engine::request_context::current_cold_compute_completeness();
    drop(scope);
    (value, local)
}

fn resolve_member_carrier(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: verter_type_engine::semantic_query::SemanticNodeId,
    tracked_dependency: bool,
    eager_context: ProjectionReductionContext,
    carrier_context: ProjectionReductionContext,
) -> Option<verter_type_engine::semantic_query::SemanticNodeId> {
    let (resolved, local) = observe_local(|| {
        dispatch.resolve_carrier_subject_node(
            node,
            if tracked_dependency {
                eager_context
            } else {
                carrier_context
            },
        )
    });
    if local.is_partial() {
        None
    } else {
        Some(resolved)
    }
}

/// This demand's fact from the completeness its own work observed.
/// An empty set is a complete answer. Callers pass that observation,
/// not a parent scope with another demand's reasons removed.
pub(super) fn demand_fact(
    inputs: verter_type_engine::semantic_query::ResultCompleteness,
) -> FactResult<()> {
    FactResult::from_demand_inputs(inputs.reasons())
}

/// The fact of one testing row or one synthesized emit row that was emitted.
///
/// Emitting the row is that demand's answer. A typed degradation stays on
/// the row (`unknown` plus the reason) and does not make the fact
/// unavailable: one member's contained inference failure must not partial
/// the file summary. A row that is not emitted publishes no fact, so it
/// cannot borrow a sibling row's status.
pub(super) fn row_derivation_fact() -> FactResult<()> {
    FactResult::complete(())
}

/// Whether a DEGRADED-SUCCESS flow return was observed while deriving this
/// surface — the precondition for the runtime lane's empty-surface
/// refusal.
///
/// A props surface with no members is a legitimate answer for
/// `defineProps<{}>()`, and only for it. Under an observed degradation the
/// same empty surface means the substrate could not type the ROOT, so
/// there is no member set at all and publishing one declares a props-less
/// component for a component that declares props.
///
/// Scoped to the two DEGRADED-SUCCESS classes. [`flow_return_no_surface_observed`]
/// is the other side of that split: a producer that yielded no member set
/// at all refuses even when a sibling arm made the assembled surface
/// non-empty. A complete negative surface is discharged as its own
/// resolution arm and never reaches here.
pub(super) fn flow_return_degradation_observed() -> bool {
    let reasons =
        verter_type_engine::request_context::current_cold_compute_completeness().reasons();
    reasons.contains(PartialReasonSet::FLOW_RETURN_UNINFERRED)
        || reasons.contains(PartialReasonSet::FLOW_RETURN_UNVERIFIED)
}

/// Whether a producer this derivation asked for yielded no member set.
///
/// Distinct from [`flow_return_degradation_observed`]: that pair leaves a
/// surface whose member set is complete, so the runtime lane refuses only
/// when the assembled option object is empty. This class has no structure
/// to address a member of. One authored intersection arm or one `interface
/// … extends` heritage clause makes the assembled surface non-empty, and
/// an emptiness check then publishes the sibling arm alone.
///
/// Sample this immediately after the root surface projection. Member-value
/// classification shares the cold scope and records the same bit when one
/// member's type has no surface; that observation degrades the member and
/// is not a missing member set.
pub(super) fn flow_return_no_surface_observed() -> bool {
    verter_type_engine::request_context::current_cold_compute_completeness()
        .reasons()
        .contains(PartialReasonSet::FLOW_RETURN_NO_SURFACE)
}

pub(super) fn expansion_kind(kind: AnalyzedMacroKind) -> MacroExpansionKind {
    match kind {
        AnalyzedMacroKind::DefineEmits => MacroExpansionKind::DefineEmits,
        AnalyzedMacroKind::DefineSlots => MacroExpansionKind::DefineSlots,
        _ => MacroExpansionKind::DefineProps,
    }
}

/// An absent declaration. A partial observation of the lookup that missed
/// is that lookup's [`failure_from_local`], not an ambient lane veto.
pub(super) fn resolution_failure() -> ProjectionFailure {
    ProjectionFailure::Unresolved(UnresolvedReason::MissingDeclaration)
}

/// `local` when that observation is partial; a missing declaration when it is not.
pub(super) fn failure_from_local(local: ResultCompleteness) -> ProjectionFailure {
    if local.is_partial() {
        failure_from_causes(local.reasons())
    } else {
        ProjectionFailure::Unresolved(UnresolvedReason::MissingDeclaration)
    }
}

/// The typed refusal of a lane whose inputs are incomplete. An abort class
/// the computation observed makes the whole production publish nothing
/// ([`verter_type_engine::semantic_query::ExecutionAbort::observed_in`]), so it has no refusal row of its own here.
pub(super) fn partial_failure() -> ProjectionFailure {
    let reasons =
        verter_type_engine::request_context::current_cold_compute_completeness().reasons();
    failure_from_causes(reasons)
}

fn failure_from_causes(reasons: PartialReasonSet) -> ProjectionFailure {
    let reason = if reasons.contains(PartialReasonSet::BUDGET_EXCEEDED)
        || reasons.contains(PartialReasonSet::OPERATION_BUDGET)
        || reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT)
        || reasons.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT)
        || reasons.contains(PartialReasonSet::DEFERRED_EVALUATION_LIMIT)
        || reasons.contains(PartialReasonSet::STRUCTURAL_FACT_DEMAND_LIMIT)
    {
        MacroPartialReason::BudgetExceeded
    } else if reasons.contains(PartialReasonSet::SAME_PATH_RECURSION)
        || reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
    {
        MacroPartialReason::Recursion
    } else {
        MacroPartialReason::IncompleteTraversal
    };
    ProjectionFailure::Partial(reason)
}

pub(super) fn macro_index(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro inventory exceeds the DTO identity space")
}

fn member_ordinal(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro member inventory exceeds the DTO identity space")
}

#[cfg(test)]
mod lane_containment_tests {
    use verter_type_engine::semantic_query::surface_resolution::NonEmptyReasons;
    use verter_type_engine::semantic_query::{
        root_narrowing_fact, FactStatus, PartialReason, PartialReasonSet, ResultCompleteness,
    };

    #[test]
    fn exact_runtime_member_is_independent_of_an_earlier_failed_fact() {
        use crate::{HostConfig, UpsertRequest, VerterHost};
        use std::sync::Arc;
        use verter_macro_dto::RuntimeConstructor;
        use verter_type_engine::request_context::{
            fold_result_completeness, ColdComputeCompletenessScope,
        };

        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = "/src/Independent.vue";
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ good: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
            let dispatch = super::ProjectSemanticDispatch::new(ctx);
            let indexed = ctx.ensure_indexed_ready_serve(canonical).unwrap().indexed;
            let mac = &indexed.script_analysis.as_ref().unwrap().macros[0];
            let owner = super::build_owner_decl_identity(ctx, canonical, mac.owner);
            let subject = dispatch.broad_runtime_subject_for_macro(&owner, 0).unwrap();
            let _scope = ColdComputeCompletenessScope::enter();
            fold_result_completeness(ResultCompleteness::partial(
                PartialReasonSet::BUDGET_EXCEEDED,
            ));
            let classification = super::classify_runtime(
                &dispatch,
                subject.member(Arc::from("good")),
                &mut super::VueMacroCodegenCounters::default(),
            )
            .expect("an exact sibling's classification owns its guarantee");
            assert_eq!(
                classification.constructors.as_slice(),
                &[RuntimeConstructor::String]
            );
        });
    }

    /// The two flow-return NO-VALUE and DEGRADED-SUCCESS classes are
    /// DISTINCT bits, not one bit read two ways.
    ///
    /// Mutation recipe: aliasing `FLOW_RETURN_NO_SURFACE` back onto
    /// `FLOW_RETURN_UNVERIFIED` makes both `contains` assertions here fire
    /// and silently restores the per-surface guard's blind spot.
    #[test]
    fn the_no_surface_class_is_not_a_degraded_success_class() {
        assert!(
            !PartialReasonSet::FLOW_RETURN_DEGRADED
                .contains(PartialReasonSet::FLOW_RETURN_NO_SURFACE),
            "there is no structure to address a member of, so the set a consumer reads to ask \
             'may I still select a member' must exclude it"
        );
        assert!(
            !PartialReasonSet::FLOW_RETURN_NO_SURFACE
                .contains(PartialReasonSet::FLOW_RETURN_UNVERIFIED),
            "and the two must not alias"
        );
    }

    #[test]
    fn root_narrowing_does_not_borrow_a_sibling_status() {
        let open = FactStatus::Unavailable(NonEmptyReasons::of(PartialReason::FlowReturnNoSurface));
        assert!(root_narrowing_fact(&open, &[FactStatus::Complete])
            .exact()
            .is_none());
        assert!(
            root_narrowing_fact(&FactStatus::Complete, &[FactStatus::Complete])
                .exact()
                .is_some()
        );
        let missing =
            FactStatus::Unavailable(NonEmptyReasons::of(PartialReason::MissingDependency));
        assert!(root_narrowing_fact(&FactStatus::Complete, &[missing])
            .exact()
            .is_none());
    }
}
