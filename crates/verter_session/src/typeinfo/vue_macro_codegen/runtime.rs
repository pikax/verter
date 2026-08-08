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
                if macro_projection_faulted(MacroProjectionLane::RuntimeMemberValue) {
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
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                            identity.owner,
                        ),
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
            | SemanticNodeData::Signature { .. } => return true,
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
                            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
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
    if macro_projection_faulted(MacroProjectionLane::RuntimeMemberValue) {
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
        QueryResult::Error(_) => {
            return Err(resolution_failure(MacroProjectionLane::RuntimeMemberValue))
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
        let Some(name) = member.published_name() else {
            continue;
        };
        push_emit(
            &mut rows,
            name.as_ref(),
            authored_emit_anchor(mac, payload_index, effective_index, name.as_ref()),
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
    /// The FILE-level aggregate — the producer's own `completeness`,
    /// which governs warm admission of the whole codegen artifact rather
    /// than the content of either lane's output.
    ///
    /// It contains everything BOTH lanes contain between them, because a
    /// lane that refused recorded the refusal IN its entry: the artifact
    /// is a faithful, deterministic record of that refusal and re-deriving
    /// it would produce the identical bytes. Faulting the file here
    /// instead would make every module carrying one unmodelled helper
    /// recompute its whole macro codegen on every touch, forever, to
    /// re-derive a refusal it already holds.
    File,
}

impl MacroProjectionLane {
    /// The partial classes whose observation does NOT, on its own, make
    /// this lane's output an incomplete surface.
    ///
    /// The two DEGRADED-SUCCESS classes are contained by BOTH lanes. The
    /// TSC projection is unaffected by construction (the declarations
    /// ride verbatim), and faulting on them deleted the WHOLE props
    /// projection over one class member's return type for programs the
    /// checker types without difficulty. The RUNTIME projection does read
    /// the value, but it reads it PER MEMBER and degrades per member: a
    /// member carrying the positional marker, and every member of an
    /// unverified frame, publish with `type: null` (validation and casting
    /// off), while a member the substrate typed exactly keeps its real
    /// constructor. Both classes leave a COMPLETE member set, which is
    /// what makes containing them sound.
    ///
    /// [`PartialReasonSet::FLOW_RETURN_NO_SURFACE`] is where the lanes
    /// part. It says the producer yielded no member set at all, so the
    /// TSC splice is still whole and a derived option object is missing
    /// members it cannot name. Containing it on the runtime lane and
    /// relying on a structural "is the assembled surface empty" check
    /// instead asks a per-SURFACE question of a per-CONTRIBUTION
    /// invariant: one authored intersection arm, or one `interface …
    /// extends` heritage clause, makes the surface non-empty and the
    /// missing members disappear without a diagnostic.
    ///
    /// The runtime lane still ALSO keeps the structural check, and it is
    /// not redundant with this one: a DEGRADED SUCCESS whose marker sits
    /// at the ROOT position (`{ ...new Box(), n: 1 }` — the literal fails
    /// closed on a spread source it cannot evaluate) is a contained class
    /// that nonetheless leaves no members. The class says the surface is
    /// faithful; only the surface says it is empty.
    ///
    /// Every OTHER reason class still faults on BOTH lanes: a budget edge,
    /// a cancellation, a superseded generation, an unstable view, a
    /// recursion limit, a missing dependency, a semantic-query fault, and
    /// the boolean-bridge `PROPAGATED` are all statements about the
    /// REQUEST rather than about one declaration's inference. A
    /// declaration-local budget the class-member inference records
    /// precisely still reaches the FILE-level aggregate through its own
    /// rail, so a budget-truncated class member keeps the entry `Complete`
    /// (the authored splice is intact) while the file result reports
    /// `BUDGET_EXCEEDED` and warms nothing.
    const fn contained(self) -> PartialReasonSet {
        match self {
            Self::RuntimeOptions => PartialReasonSet::FLOW_RETURN_DEGRADED,
            Self::Tsc | Self::RuntimeNames | Self::RuntimeMemberValue | Self::File => {
                PartialReasonSet::FLOW_RETURN_DEGRADED
                    .union(PartialReasonSet::FLOW_RETURN_NO_SURFACE)
            }
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

/// Whether the observed completeness FAULTS `lane`'s projection.
pub(super) fn macro_projection_faulted(lane: MacroProjectionLane) -> bool {
    !macro_projection_residual(
        crate::request_context::current_cold_compute_completeness(),
        lane,
    )
    .is_empty()
}

/// The reasons of `completeness` that FAULT `lane` — the observed set
/// minus [`MacroProjectionLane::contained`].
pub(super) fn macro_projection_residual(
    completeness: crate::semantic_query::ResultCompleteness,
    lane: MacroProjectionLane,
) -> PartialReasonSet {
    completeness.reasons().without(lane.contained())
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
    let reasons = crate::request_context::current_cold_compute_completeness().reasons();
    reasons.contains(PartialReasonSet::FLOW_RETURN_UNINFERRED)
        || reasons.contains(PartialReasonSet::FLOW_RETURN_UNVERIFIED)
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

#[cfg(test)]
mod lane_containment_tests {
    use super::{macro_projection_residual, MacroProjectionLane};
    use crate::semantic_query::{PartialReasonSet, ResultCompleteness};

    fn residual(reason: PartialReasonSet, lane: MacroProjectionLane) -> PartialReasonSet {
        macro_projection_residual(ResultCompleteness::Partial(reason), lane)
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
            assert!(
                residual(degraded, MacroProjectionLane::Tsc).is_empty(),
                "the authored splice is intact under {degraded:?}"
            );
            assert!(
                residual(degraded, MacroProjectionLane::RuntimeOptions).is_empty(),
                "{degraded:?} leaves a COMPLETE member set, so the runtime lane publishes it \
                 with the affected members' validation off rather than deleting the module"
            );
        }

        // The NO-SURFACE class: the lanes part, and they part on whether
        // the demand RENDERS an option object rather than on whether it is
        // nominally "runtime".
        let no_surface = PartialReasonSet::FLOW_RETURN_NO_SURFACE;
        assert!(
            residual(no_surface, MacroProjectionLane::Tsc).is_empty(),
            "the TSC lane splices the AUTHORED declaration, which rides verbatim whatever the \
             substrate could not compute"
        );
        assert_eq!(
            residual(no_surface, MacroProjectionLane::RuntimeOptions),
            no_surface,
            "an option-rendering demand DERIVES its output from the value: a producer that \
             yielded no member set leaves it missing members it cannot name, and no \
             structural check on the ASSEMBLED surface can see that once a sibling arm \
             contributed"
        );
        assert!(
            residual(no_surface, MacroProjectionLane::RuntimeNames).is_empty(),
            "a NAMES-ONLY (TSX / IDE) demand emits no option object, and the TSX it feeds \
             splices the AUTHORED macro call — faulting it would delete the whole file's \
             type-check surface to prevent bytes this demand never writes"
        );
        assert!(
            residual(no_surface, MacroProjectionLane::RuntimeMemberValue).is_empty(),
            "and at ONE MEMBER's classification the same class says that member's TYPE is \
             unknown, which the lane encodes as `type: null` on that member — the observation \
             is STICKY for the scope, so faulting here collapses every LATER member's \
             constructor too"
        );

        // The FILE aggregate contains both lanes' sets: a lane that
        // refused recorded the refusal IN its entry, and warming that
        // faithful record is correct.
        for contained in [
            PartialReasonSet::FLOW_RETURN_UNINFERRED,
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
            PartialReasonSet::FLOW_RETURN_NO_SURFACE,
        ] {
            assert!(
                residual(contained, MacroProjectionLane::File).is_empty(),
                "{contained:?} must not make the whole codegen artifact recompute forever to \
                 re-derive a refusal it already holds"
            );
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
                assert_eq!(
                    residual(request_class, lane),
                    request_class,
                    "{request_class:?} is a statement about the DEMAND and faults {lane:?}"
                );
                assert_eq!(
                    residual(
                        request_class.union(PartialReasonSet::FLOW_RETURN_UNINFERRED),
                        lane
                    ),
                    request_class,
                    "a mixed observation keeps its faulting class on {lane:?}"
                );
            }
        }
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
}
