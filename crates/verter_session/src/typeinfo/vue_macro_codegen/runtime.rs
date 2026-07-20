//! Runtime-bundle projection, broad-runtime classification, and the shared
//! macro ordinal / anchor / failure helpers for the Vue macro codegen producer.
//!
//! Sibling half of the parent module `impl VerterHost` orchestrator; see the
//! parent module docs for the overall producer contract.

use super::*;

/// Prove a row-local missing dependency without conflating it with an
/// authored `unknown` or with a reference nested below a constructor-bearing
/// shell. The semantic analyzer is the authority for which imported heads are
/// MEMBER-tier dependencies; this walk only follows the transparent shapes
/// that participate in broad constructor inference.
pub(super) fn direct_member_dependency_is_missing(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: crate::semantic_query::SemanticNodeId,
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
        let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => work.push((*inner, tracked_dependency)),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                work.extend(arms.iter().copied().map(|arm| (arm, tracked_dependency)));
            }
            SemanticNodeData::BareRef(_) => {
                let (name, _) = data.bare_ref_head().expect("BareRef carrier head");
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(name.as_ref());
                drop(data);
                let resolved = dispatch.resolve_carrier_subject_node(
                    node,
                    if tracked_dependency {
                        eager_context
                    } else {
                        carrier_context
                    },
                );
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return false;
                }
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::TypeOf(_) => {
                let (root, _) = data.typeof_head().expect("TypeOf carrier head");
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(root.name.as_ref());
                drop(data);
                let resolved = dispatch.resolve_carrier_subject_node(
                    node,
                    if tracked_dependency {
                        eager_context
                    } else {
                        carrier_context
                    },
                );
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return false;
                }
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::ImportType(_) => {
                drop(data);
                let resolved = dispatch.resolve_carrier_subject_node(
                    node,
                    if tracked_dependency {
                        eager_context
                    } else {
                        carrier_context
                    },
                );
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return false;
                }
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::DeclRef { identity } => {
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(identity.decl_name.as_ref());
                let identity = identity.clone();
                drop(data);
                match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                    scope: ScopeId {
                        canonical_id: Arc::clone(&identity.canonical_id),
                        owner: identity.owner,
                        local_scope: None,
                    },
                    name: Arc::clone(&identity.decl_name),
                })) {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(crate::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !crate::request_context::current_cold_compute_completeness()
                                .is_partial() =>
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
                let args = Arc::clone(args);
                drop(data);
                match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(
                            Arc::clone(&base.canonical_id),
                            base.owner,
                            Arc::clone(&base.decl_name),
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
                )) {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(crate::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !crate::request_context::current_cold_compute_completeness()
                                .is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                ..
            }) => {
                let tracked_dependency =
                    tracked_dependency || dependency_names.contains(name.as_ref());
                let canonical_id = Arc::clone(canonical_id);
                let owner = *owner;
                let name = Arc::clone(name);
                drop(data);
                match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(Arc::clone(&canonical_id), owner, name),
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
                )) {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(crate::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !crate::request_context::current_cold_compute_completeness()
                                .is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::Miss)
                if tracked_dependency
                    && !crate::request_context::current_cold_compute_completeness()
                        .is_partial() =>
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
    dispatch: &ProjectSemanticDispatch<'_>,
    mut subject: crate::semantic_query::SemanticNodeId,
) -> bool {
    let mut visited = FxHashSet::default();
    let resolution_context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    while visited.insert(subject) {
        let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, subject)
        else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Function { .. }
            | SemanticNodeData::ConstructorType { .. } => return true,
            SemanticNodeData::Alias(inner) => subject = *inner,
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::TypeOf(_) => {
                drop(data);
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
                            canonical_id: Arc::clone(&identity.canonical_id),
                            owner: identity.owner,
                            local_scope: None,
                        },
                        name: Arc::clone(&identity.decl_name),
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
                let args = Arc::clone(args);
                drop(data);
                let resolved = dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(
                            Arc::clone(&base.canonical_id),
                            base.owner,
                            Arc::clone(&base.decl_name),
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
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                ..
            }) => {
                let canonical_id = Arc::clone(canonical_id);
                let owner = *owner;
                let name = Arc::clone(name);
                drop(data);
                let resolved = dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        dispatch.type_slot_for(Arc::clone(&canonical_id), owner, name),
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
/// scope. A `false` result makes no authoritative claim: the following
/// shallow-surface demand owns root completeness and will re-observe any real
/// missing root/surface arm. A `true` result is authoritative and retains any
/// partiality encountered while reaching the non-object terminal.
pub(super) fn probe_definitely_non_object_root(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: crate::semantic_query::SemanticNodeId,
) -> bool {
    let probe_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    let definitely_non_object = is_definitely_non_object_root(dispatch, subject);
    if definitely_non_object {
        drop(probe_scope);
    } else {
        probe_scope.discard();
    }
    definitely_non_object
}

pub(super) struct RuntimeClassification {
    pub(super) constructors: OrderedRuntimeConstructors,
    pub(super) skip_check: bool,
}

pub(super) fn classify_runtime(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: BroadRuntimeSubjectLocator,
    counters: &mut VueMacroCodegenCounters,
) -> Result<RuntimeClassification, ProjectionFailure> {
    counters.runtime_classifier_calls += 1;
    let result = dispatch.execute(dispatch.broad_runtime_key_for(subject));
    if crate::request_context::current_cold_compute_completeness().is_partial() {
        return Err(partial_failure());
    }
    let classification = match result {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::BroadRuntime(classification) => classification,
            _ => {
                return Err(ProjectionFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ))
            }
        },
        QueryResult::Recursive(_) => {
            return Err(ProjectionFailure::Partial(MacroPartialReason::Recursion))
        }
        QueryResult::Error(_) => return Err(resolution_failure()),
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

pub(super) fn emit_rows(
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &TypeInfoSurface,
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    runtime_context: ProjectionReductionContext,
) -> Vec<RuntimeEmit> {
    let mut rows = Vec::new();

    // The filtered semantic surface is the sole event-membership authority.
    // `mac.emit_fields` is parser-owned anchor metadata and can include fields
    // reached through producer-addressed `@vue-ignore` heritage; seeding rows
    // from it would reintroduce an arm the runtime surface deliberately
    // removed. Use it only through `authored_emit_anchor` after a semantic
    // call-signature/member has admitted the event.
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate)
        .with_orthogonal_axes_from(runtime_context);
    for signature in surface.call_signatures.iter() {
        let Some(names) = CallableNodeView::new(dispatch, signature.node).event_names(context)
        else {
            continue;
        };
        for name in names {
            push_emit(
                &mut rows,
                name.as_ref(),
                authored_emit_anchor(mac, payload_index, effective_index, name.as_ref()),
            );
        }
    }
    for member in surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
    {
        push_emit(
            &mut rows,
            member.name.as_ref(),
            authored_emit_anchor(mac, payload_index, effective_index, member.name.as_ref()),
        );
    }
    rows.sort_by_key(|row| authored_emit_order(row.anchor));
    rows
}

/// Decide only resolved, concrete member shapes. Open/opaque member payloads
/// retain the existing conservative emit-name projection, while concrete
/// scalar/object/array payloads cannot satisfy Vue's tuple/function payload
/// contract and invalidate the enclosing macro.
pub(super) fn emits_surface_has_invalid_member(
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &TypeInfoSurface,
    context: ProjectionReductionContext,
) -> bool {
    use crate::semantic_query::{PrimitiveKind, SemanticNodeData};

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
            match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
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

fn push_emit(rows: &mut Vec<RuntimeEmit>, name: &str, anchor: MacroAnchor) {
    if rows.iter().any(|row| row.name == name) {
        return;
    }
    rows.push(RuntimeEmit {
        name: name.to_owned(),
        anchor,
    });
}

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

pub(super) fn defaults_association(
    payload_index: usize,
    defaults_index: Option<usize>,
) -> PropsDefaultsAssociation {
    defaults_index.map_or(PropsDefaultsAssociation::None, |index| {
        PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: macro_index(payload_index),
            defaults_macro_index: macro_index(index),
        }
    })
}

pub(super) fn containing_with_defaults_index(
    macros: &[AnalyzedMacro],
    inner_index: usize,
) -> Option<usize> {
    let inner = &macros[inner_index];
    macros
        .iter()
        .enumerate()
        .filter(|(_, outer)| outer.kind == AnalyzedMacroKind::WithDefaults)
        .filter(|(_, outer)| outer.span.start < inner.span.start && inner.span.end < outer.span.end)
        .min_by_key(|(_, outer)| outer.span.end.saturating_sub(outer.span.start))
        .map(|(index, _)| index)
}

pub(super) fn top_level_syntax_index(macros: &[AnalyzedMacro], effective_index: usize) -> u32 {
    let effective = &macros[effective_index];
    debug_assert!(is_top_level_macro(macros, effective_index));

    let preceding = macros
        .iter()
        .enumerate()
        .filter(|(index, _)| is_top_level_macro(macros, *index))
        .filter(|(index, mac)| {
            (mac.span.start, mac.span.end, *index)
                < (effective.span.start, effective.span.end, effective_index)
        })
        .count();
    u32::try_from(preceding).unwrap_or(u32::MAX)
}

fn is_top_level_macro(macros: &[AnalyzedMacro], candidate_index: usize) -> bool {
    let candidate = &macros[candidate_index];
    !macros.iter().enumerate().any(|(index, outer)| {
        index != candidate_index
            && outer.span.start < candidate.span.start
            && candidate.span.end < outer.span.end
    })
}

pub(super) fn is_codegen_macro(kind: AnalyzedMacroKind) -> bool {
    matches!(
        kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineModel
    )
}

pub(super) fn expansion_kind(kind: AnalyzedMacroKind) -> MacroExpansionKind {
    match kind {
        AnalyzedMacroKind::DefineEmits => MacroExpansionKind::DefineEmits,
        AnalyzedMacroKind::DefineSlots => MacroExpansionKind::DefineSlots,
        _ => MacroExpansionKind::DefineProps,
    }
}

pub(super) fn resolution_failure() -> ProjectionFailure {
    if crate::request_context::current_cold_compute_completeness().is_partial() {
        partial_failure()
    } else {
        ProjectionFailure::Unresolved(UnresolvedReason::MissingDeclaration)
    }
}

pub(super) fn partial_failure() -> ProjectionFailure {
    let reasons = crate::request_context::current_cold_compute_completeness().reasons();
    let reason = if reasons.contains(PartialReasonSet::CANCELLED) {
        MacroPartialReason::Cancelled
    } else if reasons.contains(PartialReasonSet::SUPERSEDED_GENERATION) {
        MacroPartialReason::SupersededGeneration
    } else if reasons.contains(PartialReasonSet::UNSTABLE_STATE) {
        MacroPartialReason::UnstableState
    } else if reasons.contains(PartialReasonSet::BUDGET_EXCEEDED)
        || reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT)
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

pub(super) fn fact_footprint(finalise: FactReadSetFinalise) -> (Vec<String>, bool) {
    let (facts, cacheable) = match finalise {
        FactReadSetFinalise::Ok(facts) => (Some(facts), true),
        FactReadSetFinalise::NonCacheable(facts) => (Some(facts), false),
        FactReadSetFinalise::Overflow => (None, false),
    };
    let mut canonicals = BTreeSet::new();
    if let Some(facts) = facts {
        for fact in facts.iter() {
            if let Some(canonical) = fact.canonical_id() {
                canonicals.insert(canonical.to_owned());
            }
        }
    }
    (canonicals.into_iter().collect(), cacheable)
}
