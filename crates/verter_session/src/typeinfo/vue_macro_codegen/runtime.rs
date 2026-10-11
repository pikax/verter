//! Runtime-bundle projection, broad-runtime classification, and the shared
//! macro ordinal / anchor / failure helpers for the Vue macro codegen producer.
//!
//! Sibling half of the parent module `impl VerterHost` orchestrator; see the
//! parent module docs for the overall producer contract.

use super::*;
use verter_type_engine::semantic_query::{
    root_narrowing_fact, FactResult, FactStatus, PartialReason,
};

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
        let Some(data) =
            verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), node)
        else {
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
                if macro_projection_faulted(MacroProjectionLane::RuntimeMemberValue) {
                    return false;
                }
                if resolved != node {
                    work.push((resolved, tracked_dependency));
                }
            }
            SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_) => {
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
                if macro_projection_faulted(MacroProjectionLane::RuntimeMemberValue) {
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
                if macro_projection_faulted(MacroProjectionLane::RuntimeMemberValue) {
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
                        binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                            identity.owner,
                        ),
                    },
                    name: Arc::clone(&identity.decl_name),
                })) {
                    QueryResult::Value(resolved) if resolved.value != node => {
                        work.push((resolved.value, tracked_dependency));
                    }
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !verter_type_engine::request_context::current_cold_compute_completeness()
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
                    verter_type_engine::semantic_query::InstantiateKey::new(
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
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !verter_type_engine::request_context::current_cold_compute_completeness()
                                .is_partial() =>
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
                let canonical_id = Arc::clone(canonical_id);
                let owner = *owner;
                let name = Arc::clone(name);
                drop(data);
                match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    verter_type_engine::semantic_query::InstantiateKey::new(
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
                    QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
                        if tracked_dependency
                            && !verter_type_engine::request_context::current_cold_compute_completeness()
                                .is_partial() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            SemanticNodeData::Opaque(verter_type_engine::semantic_query::QueryError::Miss)
                if tracked_dependency
                    && !verter_type_engine::request_context::current_cold_compute_completeness(
                    )
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
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    mut subject: verter_type_engine::semantic_query::SemanticNodeId,
) -> bool {
    let mut visited = FxHashSet::default();
    let resolution_context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    while visited.insert(subject) {
        let Some(data) =
            verter_type_engine::project_semantic_dispatch::node_data_for(dispatch.graph(), subject)
        else {
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
                            canonical_id: Arc::clone(&identity.canonical_id),
                            owner: identity.owner,
                            local_scope: None,
                            binder_scope_id:
                                verter_type_engine::semantic_query::BinderScopeId::file_scope(
                                    identity.owner,
                                ),
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
                    verter_type_engine::semantic_query::InstantiateKey::new(
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
            SemanticNodeData::Opaque(
                verter_type_engine::semantic_query::QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    ..
                },
            ) => {
                let canonical_id = Arc::clone(canonical_id);
                let owner = *owner;
                let name = Arc::clone(name);
                drop(data);
                let resolved = dispatch.execute_type_node(SemanticQueryKey::Instantiate(
                    verter_type_engine::semantic_query::InstantiateKey::new(
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
            match verter_type_engine::project_semantic_dispatch::node_data_for(
                dispatch.graph(),
                node,
            )
            .as_deref()
            {
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

/// WHICH macro codegen consumer is asking whether an observed partial
/// faults it.
///
/// The consumers read the same observation and are broken by different
/// parts of it, so the containment set is a property of the CONSUMER and
/// cannot be a single shared constant. Naming the consumer at every fault
/// site is the point: a new site does not compile until it has said which
/// output it is protecting.
///
/// The axis is not "TSC vs runtime" — it is whether the consumer DERIVES
/// its output from the resolved value. A names-only runtime demand does
/// not, which is why it sits beside the TSC lane rather than beside the
/// option-rendering one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MacroProjectionLane {
    /// The TSC projection: it emits the AUTHORED declarations verbatim
    /// into the generated TSX and splices the AUTHORED type argument, and
    /// the external checker computes the member types itself.
    Tsc,
    /// A RUNTIME demand that RENDERS the option objects, asked at the
    /// point that derives the MEMBER SET: it DERIVES `props: {…}` /
    /// `emits: […]` from the resolved value, so a producer that yielded
    /// no member set leaves members the module Vue executes does not
    /// declare.
    RuntimeOptions,
    /// The SAME option-rendering demand, asked while classifying ONE
    /// member's constructors.
    ///
    /// The class means something different here, and reading it the same
    /// way is an over-degradation with teeth. At the member-SET point a
    /// no-surface observation says members are missing; at THIS point it
    /// says one member's TYPE is unknown, and the lane already has an
    /// exact encoding for that — `type: null`, on that member alone. The
    /// observed completeness is STICKY for the scope, so faulting here
    /// would let the FIRST member whose value has no surface collapse
    /// every LATER member's constructor too: `defineProps<{ a:
    /// ReturnType<typeof unmodelled>; b: string }>()` publishes
    /// `b: { type: null }` instead of `b: { type: String }`, for a `b` the
    /// same tree types exactly.
    RuntimeMemberValue,
    /// A NAMES-ONLY runtime demand (the TSX / IDE compile). It renders no
    /// option object at all, and the TSX it feeds splices the AUTHORED
    /// macro call for the external checker — so it sits on the TSC side
    /// of the containment axis, and faulting it would delete the whole
    /// file's type-check surface to prevent an option object this demand
    /// never emits.
    RuntimeNames,
    /// Request-integrity inputs of a file-sized demand. Production does
    /// not construct this lane: the artifact summary is the published
    /// facts. The predicate stays so a request-integrity reason is still
    /// an input of every demand, including this one.
    #[allow(dead_code)]
    File,
}

impl MacroProjectionLane {
    /// Whether `reason` is an input this demand owns.
    ///
    /// A degraded flow return is a value-local fact. It is not an input of
    /// these demands. A missing surface and an undecided conditional are
    /// inputs only of the demand that renders option objects from the value.
    /// Every request-integrity reason is an input of every demand. The caller
    /// passes only the owned set to [`demand_fact`]; nothing is stripped there.
    fn owns_reason(self, reason: PartialReason) -> bool {
        match reason {
            PartialReason::FlowReturnUninferred | PartialReason::FlowReturnUnverified => false,
            PartialReason::FlowReturnNoSurface | PartialReason::UndecidedConditional => {
                matches!(self, Self::RuntimeOptions)
            }
            PartialReason::BudgetExceeded
            | PartialReason::Cancelled
            | PartialReason::SupersededGeneration
            | PartialReason::UnstableState
            | PartialReason::SamePathRecursion
            | PartialReason::WalkerFatal
            | PartialReason::Propagated
            | PartialReason::DeferredEvaluationLimit
            | PartialReason::StructuralFactDemandLimit
            | PartialReason::SemanticQueryFault
            | PartialReason::MissingSemanticNodeData
            | PartialReason::ProjectionWorkLimit
            | PartialReason::ConnectedQueryDepthLimit
            | PartialReason::MissingDependency
            | PartialReason::ConnectedMemoryLimit
            | PartialReason::OperationBudget => true,
        }
    }

    /// The runtime lane for a demand that does (or does not) render the
    /// option objects — the ONE derivation of that distinction, taken
    /// from `VueMacroCodegenDemand::wants_runtime_constructors`.
    pub(super) const fn runtime(renders_options: bool) -> Self {
        if renders_options {
            Self::RuntimeOptions
        } else {
            Self::RuntimeNames
        }
    }
}

/// The completeness whose reasons are exactly the inputs `lane` owns.
/// Reasons that belong to another demand are omitted here, before
/// [`demand_fact`], and are not stripped inside it.
pub(super) fn lane_inputs(
    lane: MacroProjectionLane,
    observed: verter_type_engine::semantic_query::ResultCompleteness,
) -> verter_type_engine::semantic_query::ResultCompleteness {
    let mut inputs = PartialReasonSet::empty();
    for reason in observed.reasons().iter() {
        if lane.owns_reason(reason) {
            inputs = inputs.union(reason.bit());
        }
    }
    if inputs.is_empty() {
        verter_type_engine::semantic_query::ResultCompleteness::Complete
    } else {
        verter_type_engine::semantic_query::ResultCompleteness::partial(inputs)
    }
}

/// This demand's fact from the inputs its own work observed.
/// An empty input set is a complete answer. The caller has already omitted
/// every reason that belongs to another demand.
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

/// Whether the observed completeness leaves `lane`'s fact inexact.
pub(super) fn macro_projection_faulted(lane: MacroProjectionLane) -> bool {
    let observed = verter_type_engine::request_context::current_cold_compute_completeness();
    demand_fact(lane_inputs(lane, observed)).exact().is_none()
}

/// Status of `lane` against the completeness active on this thread.
pub(super) fn lane_fact_status(lane: MacroProjectionLane) -> FactStatus {
    let observed = verter_type_engine::request_context::current_cold_compute_completeness();
    demand_fact(lane_inputs(lane, observed)).status()
}

/// Whether a DEGRADED flow return was observed while deriving this
/// surface — the precondition for the runtime lane's empty-surface
/// refusal.
///
/// A props surface with no members is a legitimate answer for
/// `defineProps<{}>()`, and only for it. Under an observed degradation the
/// same empty surface means the substrate could not type the ROOT, so
/// there is no member set at all and publishing one declares a props-less
/// component for a component that declares props.
///
/// Scoped to the two DEGRADED-SUCCESS classes, which are exactly the ones
/// the runtime lane CONTAINS: a no-surface observation already faulted the
/// lane through [`macro_projection_faulted`] and never reaches here.
pub(super) fn flow_return_degradation_observed() -> bool {
    let reasons =
        verter_type_engine::request_context::current_cold_compute_completeness().reasons();
    reasons.contains(PartialReasonSet::FLOW_RETURN_UNINFERRED)
        || reasons.contains(PartialReasonSet::FLOW_RETURN_UNVERIFIED)
}

pub(super) fn expansion_kind(kind: AnalyzedMacroKind) -> MacroExpansionKind {
    match kind {
        AnalyzedMacroKind::DefineEmits => MacroExpansionKind::DefineEmits,
        AnalyzedMacroKind::DefineSlots => MacroExpansionKind::DefineSlots,
        _ => MacroExpansionKind::DefineProps,
    }
}

/// An absent declaration, reported as `lane` sees it: a partial when the
/// observation already faults that lane, a plain missing declaration
/// otherwise.
pub(super) fn resolution_failure(lane: MacroProjectionLane) -> ProjectionFailure {
    if macro_projection_faulted(lane) {
        partial_failure()
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
    use super::{demand_fact, lane_inputs, MacroProjectionLane};
    use verter_type_engine::semantic_query::surface_resolution::NonEmptyReasons;
    use verter_type_engine::semantic_query::{
        root_narrowing_fact, summarize, FactResult, FactStatus, PartialReason, PartialReasonSet,
        ResultCompleteness,
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

    fn fact(reason: PartialReasonSet, lane: MacroProjectionLane) -> FactResult<()> {
        demand_fact(lane_inputs(lane, ResultCompleteness::Partial(reason)))
    }

    fn assert_exact(reason: PartialReasonSet, lane: MacroProjectionLane) {
        assert!(
            fact(reason, lane).exact().is_some(),
            "{reason:?} is not an input of {lane:?}"
        );
    }

    fn assert_faults(reason: PartialReasonSet, lane: MacroProjectionLane) {
        let result = fact(reason, lane);
        assert!(result.exact().is_none(), "{reason:?} must fault {lane:?}");
        assert_eq!(result.causes().map(|causes| causes.get()), Some(reason));
    }

    /// The lanes disagree on EXACTLY one class, and the disagreement is
    /// the whole point of parameterising the predicate.
    ///
    /// A no-surface producer contributed no member set, so a lane that
    /// DERIVES its output from the value is missing members it cannot
    /// name and must fault; a lane that splices the AUTHORED declaration
    /// for an external checker is unaffected.
    ///
    /// Mutation recipe: giving `Runtime` the same set as `Tsc` (a single
    /// shared containment constant — which is exactly what this predicate
    /// replaced) passes every degraded-success row here and fails the
    /// no-surface row, and at the public boundary republishes every row of
    /// `a_no_surface_flow_return_refuses_even_when_a_sibling_arm_contributes`.
    #[test]
    fn only_the_no_surface_class_separates_the_two_macro_codegen_lanes() {
        // The DEGRADED-SUCCESS pair: a complete member set, contained by
        // BOTH lanes.
        for degraded in [
            PartialReasonSet::FLOW_RETURN_UNINFERRED,
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        ] {
            assert_exact(degraded, MacroProjectionLane::Tsc);
            assert_exact(degraded, MacroProjectionLane::RuntimeOptions);
        }

        // The NO-SURFACE class: the lanes part, and they part on whether
        // the demand RENDERS an option object rather than on whether it is
        // nominally "runtime".
        let no_surface = PartialReasonSet::FLOW_RETURN_NO_SURFACE;
        assert_exact(no_surface, MacroProjectionLane::Tsc);
        assert_faults(no_surface, MacroProjectionLane::RuntimeOptions);
        assert_exact(no_surface, MacroProjectionLane::RuntimeNames);
        assert_exact(no_surface, MacroProjectionLane::RuntimeMemberValue);

        // The file summary is the published facts, not a filter that drops
        // the runtime refusal. A no-surface runtime fact stays in it. A
        // degraded-success class is not an input of either demand.
        let no_surface = PartialReasonSet::FLOW_RETURN_NO_SURFACE;
        let runtime = fact(no_surface, MacroProjectionLane::RuntimeOptions);
        let tsc = fact(no_surface, MacroProjectionLane::Tsc);
        assert!(runtime.exact().is_none());
        assert!(tsc.exact().is_some());
        assert!(
            summarize([&runtime.status(), &tsc.status()]).is_partial(),
            "the summary keeps the published runtime refusal"
        );
        for contained in [
            PartialReasonSet::FLOW_RETURN_UNINFERRED,
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        ] {
            let runtime = fact(contained, MacroProjectionLane::RuntimeOptions);
            let tsc = fact(contained, MacroProjectionLane::Tsc);
            assert!(runtime.exact().is_some() && tsc.exact().is_some());
            assert!(!summarize([&runtime.status(), &tsc.status()]).is_partial());
        }

        // CONTROL — every class that is a statement about the REQUEST
        // faults all three, and a mixed observation is governed by its
        // worst class.
        for request_class in [
            PartialReasonSet::BUDGET_EXCEEDED,
            PartialReasonSet::CANCELLED,
            PartialReasonSet::SUPERSEDED_GENERATION,
            PartialReasonSet::UNSTABLE_STATE,
            PartialReasonSet::SAME_PATH_RECURSION,
            PartialReasonSet::MISSING_DEPENDENCY,
            PartialReasonSet::SEMANTIC_QUERY_FAULT,
            PartialReasonSet::PROPAGATED,
        ] {
            for lane in [
                MacroProjectionLane::Tsc,
                MacroProjectionLane::RuntimeOptions,
                MacroProjectionLane::RuntimeNames,
                MacroProjectionLane::RuntimeMemberValue,
                MacroProjectionLane::File,
            ] {
                assert_faults(request_class, lane);
                let mixed = fact(
                    request_class.union(PartialReasonSet::FLOW_RETURN_UNINFERRED),
                    lane,
                );
                assert_eq!(
                    mixed.causes().map(|causes| causes.get()),
                    Some(request_class),
                    "a mixed observation keeps its faulting class on {lane:?}"
                );
            }
        }
    }

    /// An undecided conditional parts the lanes as a missing surface does:
    /// the authored splice, a names-only demand, one member's
    /// classification and the file aggregate contain it, and a demand that
    /// renders the option object from the value faults on it.
    #[test]
    fn an_undecided_conditional_parts_the_lanes_as_a_missing_surface_does() {
        let undecided = PartialReasonSet::UNDECIDED_CONDITIONAL;
        for lane in [
            MacroProjectionLane::Tsc,
            MacroProjectionLane::RuntimeNames,
            MacroProjectionLane::RuntimeMemberValue,
            MacroProjectionLane::File,
        ] {
            assert_exact(undecided, lane);
        }
        assert_faults(undecided, MacroProjectionLane::RuntimeOptions);
        assert!(
            !PartialReasonSet::FLOW_RETURN_DEGRADED.contains(undecided)
                && !PartialReasonSet::FLOW_RETURN_NO_SURFACE.contains(undecided),
            "the class is its own bit"
        );
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
    fn an_authored_splice_stays_exact_when_the_runtime_claim_is_not() {
        let no_surface = PartialReasonSet::FLOW_RETURN_NO_SURFACE;
        assert!(fact(no_surface, MacroProjectionLane::Tsc).exact().is_some());
        assert!(fact(no_surface, MacroProjectionLane::RuntimeOptions)
            .exact()
            .is_none());
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
