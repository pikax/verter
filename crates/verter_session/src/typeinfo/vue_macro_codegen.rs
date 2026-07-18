//! Per-SFC Vue macro codegen projection.
//!
//! This module is the TypeInfo-owned semantic producer between macro argument
//! analysis and compiler-facing DTOs. It owns no durable aggregate cache and
//! submits no scheduler work: one invocation inventories one already-indexed
//! SFC, reuses the canonical macro payload carrier, and independently fulfills
//! runtime and TSC demands.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroPartialReason, MacroRuntimeBundle,
    MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscBundle, MacroTscEntry,
    MacroTscOutcome, MacroTscProjection, ModelRuntimeShape, OrderedRuntimeConstructors,
    PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeEmit, RuntimeProp,
    RuntimeRootShape, SynthesizedRowKind, TscSpliceText, UnresolvedReason, UnsupportedReason,
};
use verter_semantic::analysis::component_meta::MacroExpansionKind;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::meta_resolve::callable_view::CallableNodeView;
use crate::meta_resolve::projectors::{build_owner_decl_identity, resolve_macro_payload};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::{FactReadSetFinalise, ResolverContext};
use crate::semantic_query::{
    BroadRuntimeContext, BroadRuntimeKind, PartialReasonSet, PathSegment, ProjectionMode,
    ProjectionReductionContext, QueryResult, ResultCompleteness, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryValue, SurfaceProvenanceContext,
};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::VerterHost;

/// Which independently materializable handoff bundle the caller demands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VueMacroCodegenDemand {
    /// Runtime names, optionality, and broad constructors only.
    Runtime,
    /// Terminal TSC/IDE splice text only.
    Tsc,
    /// Both independent bundles in one inventory pass.
    RuntimeAndTsc,
}

impl VueMacroCodegenDemand {
    const fn wants_runtime(self) -> bool {
        matches!(self, Self::Runtime | Self::RuntimeAndTsc)
    }

    const fn wants_tsc(self) -> bool {
        matches!(self, Self::Tsc | Self::RuntimeAndTsc)
    }
}

/// Deterministic work counters for one producer invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VueMacroCodegenCounters {
    /// Always one for an output returned by this producer.
    pub producer_invocations: u32,
    /// Empty-path Shallow root-surface demands.
    pub root_shallow_demands: u32,
    /// In-process `ClassifyBroadRuntime` demands.
    pub runtime_classifier_calls: u32,
    /// Terminal TypeInfo display materializations for TSC.
    pub tsc_materializations: u32,
    /// Always zero: this producer never submits scheduler work.
    pub scheduler_submissions: u32,
}

/// One per-call, non-retained semantic handoff for an SFC.
#[derive(Debug, Clone)]
pub(crate) struct VueMacroCodegenOutput {
    /// Runtime bundle only when demanded.
    pub runtime: Option<Arc<MacroRuntimeBundle>>,
    /// TSC bundle only when demanded.
    pub tsc: Option<Arc<MacroTscBundle>>,
    /// Sorted, unique file canonicals observed by the per-call fact tracer.
    pub transitive_canonicals: Vec<String>,
    /// Typed structural completeness observed during this invocation.
    pub completeness: ResultCompleteness,
    /// Whether the fact footprint was bounded and based only on publishable reads.
    pub facts_cacheable: bool,
    /// Deterministic work counters.
    pub counters: VueMacroCodegenCounters,
}

#[derive(Debug, Clone, Copy)]
enum ProjectionFailure {
    Partial(MacroPartialReason),
    Unresolved(UnresolvedReason),
    Unsupported(UnsupportedReason),
}

impl ProjectionFailure {
    fn runtime(self) -> MacroRuntimeOutcome {
        match self {
            Self::Partial(reason) => MacroRuntimeOutcome::Partial(MacroFailure::new(reason, None)),
            Self::Unresolved(reason) => {
                MacroRuntimeOutcome::Unresolved(MacroFailure::new(reason, None))
            }
            Self::Unsupported(reason) => {
                MacroRuntimeOutcome::Unsupported(MacroFailure::new(reason, None))
            }
        }
    }

    fn tsc(self) -> MacroTscOutcome {
        match self {
            Self::Partial(reason) => MacroTscOutcome::Partial(MacroFailure::new(reason, None)),
            Self::Unresolved(reason) => {
                MacroTscOutcome::Unresolved(MacroFailure::new(reason, None))
            }
            Self::Unsupported(reason) => {
                MacroTscOutcome::Unsupported(MacroFailure::new(reason, None))
            }
        }
    }
}

struct ProducerState {
    runtime_entries: Vec<MacroRuntimeEntry>,
    tsc_entries: Vec<MacroTscEntry>,
    counters: VueMacroCodegenCounters,
    completeness: ResultCompleteness,
}

impl VerterHost {
    /// Produce the requested Vue macro codegen bundle from one request-bound
    /// resolver context.
    ///
    /// The result is intentionally not retained as an aggregate graph-id
    /// cache. Underlying TypeInfo semantic queries retain their own canonical
    /// memo entries and singleflight behavior.
    pub(crate) fn produce_vue_macro_codegen_with_ctx(
        &self,
        ctx: &dyn ResolverContext,
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> VueMacroCodegenOutput {
        let (state, finalise) = crate::fact_signature_helpers::install_fact_tracer(self, || {
            let _completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
            let mut state = self.produce_vue_macro_codegen_inner(ctx, owner_canonical, demand);
            state.completeness = crate::request_context::current_cold_compute_completeness();
            state
        });

        let (transitive_canonicals, facts_cacheable) = fact_footprint(finalise);
        VueMacroCodegenOutput {
            runtime: demand.wants_runtime().then(|| {
                Arc::new(MacroRuntimeBundle {
                    entries: state.runtime_entries,
                })
            }),
            tsc: demand.wants_tsc().then(|| {
                Arc::new(MacroTscBundle {
                    entries: state.tsc_entries,
                })
            }),
            transitive_canonicals,
            completeness: state.completeness,
            facts_cacheable,
            counters: state.counters,
        }
    }

    fn produce_vue_macro_codegen_inner(
        &self,
        ctx: &dyn ResolverContext,
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> ProducerState {
        let mut state = ProducerState {
            runtime_entries: Vec::new(),
            tsc_entries: Vec::new(),
            counters: VueMacroCodegenCounters {
                producer_invocations: 1,
                ..VueMacroCodegenCounters::default()
            },
            completeness: ResultCompleteness::Complete,
        };

        let Some(serve) = ctx.ensure_indexed_ready_serve(owner_canonical) else {
            return state;
        };
        let Some(script_analysis) = serve.indexed.script_analysis.as_ref() else {
            return state;
        };
        let macros = &script_analysis.macros;
        let owner = build_owner_decl_identity(ctx, owner_canonical);
        let dispatch = ProjectSemanticDispatch::new(ctx);

        for (payload_index, mac) in macros.iter().enumerate() {
            if mac.kind == AnalyzedMacroKind::WithDefaults || !mac.is_type_based {
                continue;
            }

            let defaults_index = (mac.kind == AnalyzedMacroKind::DefineProps)
                .then(|| containing_with_defaults_index(macros, payload_index))
                .flatten();
            let effective_index = defaults_index.unwrap_or(payload_index);
            let macro_index = macro_index(effective_index);

            if !is_codegen_macro(mac.kind) {
                if demand.wants_runtime() {
                    state.runtime_entries.push(MacroRuntimeEntry {
                        macro_index,
                        outcome: ProjectionFailure::Unsupported(UnsupportedReason::MacroKind)
                            .runtime(),
                    });
                }
                if demand.wants_tsc() {
                    state.tsc_entries.push(MacroTscEntry {
                        macro_index,
                        outcome: ProjectionFailure::Unsupported(UnsupportedReason::MacroKind).tsc(),
                    });
                }
                continue;
            }

            if mac.parsed_type_argument.is_none() {
                let failure = ProjectionFailure::Unresolved(UnresolvedReason::MissingTypeArgument);
                if demand.wants_runtime() {
                    state.runtime_entries.push(MacroRuntimeEntry {
                        macro_index,
                        outcome: failure.runtime(),
                    });
                }
                if demand.wants_tsc() {
                    state.tsc_entries.push(MacroTscEntry {
                        macro_index,
                        outcome: failure.tsc(),
                    });
                }
                continue;
            }

            let mut diagnostics = Vec::new();
            let payload = resolve_macro_payload(
                &dispatch,
                &owner,
                owner_canonical,
                payload_index,
                mac,
                mac.kind,
                expansion_kind(mac.kind),
                &mut diagnostics,
            );

            if demand.wants_runtime() {
                let outcome = match payload {
                    Some(payload) => match mac.kind {
                        AnalyzedMacroKind::DefineProps => self.project_runtime_props(
                            ctx,
                            &dispatch,
                            payload,
                            mac,
                            payload_index,
                            effective_index,
                            defaults_index,
                            &mut state.counters,
                        ),
                        AnalyzedMacroKind::DefineEmits => self.project_runtime_emits(
                            ctx,
                            &dispatch,
                            payload,
                            mac,
                            payload_index,
                            effective_index,
                            &mut state.counters,
                        ),
                        AnalyzedMacroKind::DefineModel => self.project_runtime_model(
                            &dispatch,
                            payload,
                            mac,
                            effective_index,
                            &mut state.counters,
                        ),
                        _ => unreachable!("codegen macro filter is exhaustive"),
                    },
                    None => resolution_failure().runtime(),
                };
                state.runtime_entries.push(MacroRuntimeEntry {
                    macro_index,
                    outcome,
                });
            }

            if demand.wants_tsc() {
                let outcome = match payload {
                    Some(payload) => {
                        state.counters.tsc_materializations += 1;
                        let rendered =
                            crate::typeinfo::raise::render_node_display_with_ctx(ctx, payload);
                        if crate::request_context::current_cold_compute_completeness().is_partial()
                        {
                            partial_failure().tsc()
                        } else {
                            match rendered {
                                Some(text) => MacroTscOutcome::Complete(match mac.kind {
                                    AnalyzedMacroKind::DefineProps => MacroTscProjection::Props {
                                        splice: TscSpliceText::new(text),
                                    },
                                    AnalyzedMacroKind::DefineEmits => MacroTscProjection::Emits {
                                        splice: TscSpliceText::new(text),
                                    },
                                    AnalyzedMacroKind::DefineModel => MacroTscProjection::Model {
                                        splice: TscSpliceText::new(text),
                                    },
                                    _ => unreachable!("codegen macro filter is exhaustive"),
                                }),
                                None => ProjectionFailure::Unsupported(
                                    UnsupportedReason::SemanticConstruct,
                                )
                                .tsc(),
                            }
                        }
                    }
                    None => resolution_failure().tsc(),
                };
                state.tsc_entries.push(MacroTscEntry {
                    macro_index,
                    outcome,
                });
            }
        }

        state
    }

    #[allow(clippy::too_many_arguments)]
    fn project_runtime_props(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        mac: &AnalyzedMacro,
        payload_index: usize,
        effective_index: usize,
        defaults_index: Option<usize>,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        counters.root_shallow_demands += 1;
        let surface = self.project_shallow_surface_graph_only(
            ctx,
            dispatch,
            payload,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
            ),
            None,
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        let Some(surface) = surface else {
            return MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                root_shape: RuntimeRootShape::NonObject,
                defaults: defaults_association(defaults_index),
                props: Vec::new(),
            }));
        };

        let mut props = Vec::new();
        for member in surface
            .members
            .iter()
            .filter(|member| member.visibility.is_public())
        {
            let classification = match classify_runtime(dispatch, member.value, counters) {
                Ok(classification) => classification,
                Err(failure) => return failure.runtime(),
            };
            props.push(RuntimeProp {
                name: member.name.as_ref().to_owned(),
                optional: member.optional,
                skip_check: classification.skip_check,
                constructors: classification.constructors,
                anchor: member_anchor(mac, payload_index, effective_index, member.name.as_ref()),
            });
        }

        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
            root_shape: RuntimeRootShape::ObjectLike,
            defaults: defaults_association(defaults_index),
            props,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn project_runtime_emits(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        mac: &AnalyzedMacro,
        payload_index: usize,
        effective_index: usize,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        counters.root_shallow_demands += 1;
        let surface = self.project_shallow_surface_graph_only(
            ctx,
            dispatch,
            payload,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::Structural,
            ),
            None,
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        let Some(surface) = surface else {
            return ProjectionFailure::Unresolved(UnresolvedReason::NonObjectRoot).runtime();
        };

        let emits = emit_rows(dispatch, &surface, mac, payload_index, effective_index);
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(emits))
    }

    fn project_runtime_model(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        mac: &AnalyzedMacro,
        effective_index: usize,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        let classification = match classify_runtime(dispatch, payload, counters) {
            Ok(classification) => classification,
            Err(failure) => return failure.runtime(),
        };
        let macro_index = macro_index(effective_index);
        let name = mac.model_name.as_deref().unwrap_or("modelValue");
        let modifiers_name = if name == "modelValue" {
            "modelModifiers".to_owned()
        } else {
            format!("{name}Modifiers")
        };
        let optional = mac
            .prop_fields
            .first()
            .is_none_or(|field| field.is_optional);

        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(ModelRuntimeShape {
            prop: RuntimeProp {
                name: name.to_owned(),
                optional,
                constructors: classification.constructors,
                skip_check: classification.skip_check,
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelProp,
                },
            },
            update_event: RuntimeEmit {
                name: format!("update:{name}"),
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelUpdateEvent,
                },
            },
            modifiers_prop: RuntimeProp {
                name: modifiers_name,
                optional: true,
                constructors: OrderedRuntimeConstructors::default(),
                skip_check: false,
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelModifiersProp,
                },
            },
        }))
    }
}

struct RuntimeClassification {
    constructors: OrderedRuntimeConstructors,
    skip_check: bool,
}

fn classify_runtime(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: crate::semantic_query::SemanticNodeId,
    counters: &mut VueMacroCodegenCounters,
) -> Result<RuntimeClassification, ProjectionFailure> {
    counters.runtime_classifier_calls += 1;
    let result = dispatch.execute(SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: BroadRuntimeContext::default(),
    });
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
    let constructors = OrderedRuntimeConstructors::from_ordered(
        classification
            .kinds()
            .iter()
            .copied()
            .map(runtime_constructor),
    );
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

fn emit_rows(
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &TypeInfoSurface,
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
) -> Vec<RuntimeEmit> {
    let mut rows = Vec::new();

    for (ordinal, field) in mac.emit_fields.iter().enumerate() {
        push_emit(
            &mut rows,
            field.name.as_str(),
            MacroAnchor::Authored {
                macro_index: macro_index(effective_index),
                member_ordinal: Some(AuthoredMemberOrdinal::new(member_ordinal(ordinal))),
            },
        );
    }

    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
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
    rows
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

fn member_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    name: &str,
) -> MacroAnchor {
    let Some(ordinal) = mac.prop_fields.iter().position(|field| field.name == name) else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(effective_index),
        member_ordinal: Some(AuthoredMemberOrdinal::new(member_ordinal(ordinal))),
    }
}

fn authored_emit_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    name: &str,
) -> MacroAnchor {
    let Some(ordinal) = mac.emit_fields.iter().position(|field| field.name == name) else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(effective_index),
        member_ordinal: Some(AuthoredMemberOrdinal::new(member_ordinal(ordinal))),
    }
}

fn defaults_association(defaults_index: Option<usize>) -> PropsDefaultsAssociation {
    defaults_index.map_or(PropsDefaultsAssociation::None, |index| {
        PropsDefaultsAssociation::WithDefaults {
            defaults_macro_index: macro_index(index),
        }
    })
}

fn containing_with_defaults_index(macros: &[AnalyzedMacro], inner_index: usize) -> Option<usize> {
    let inner = &macros[inner_index];
    macros
        .iter()
        .enumerate()
        .filter(|(_, outer)| outer.kind == AnalyzedMacroKind::WithDefaults)
        .filter(|(_, outer)| outer.span.start < inner.span.start && inner.span.end < outer.span.end)
        .min_by_key(|(_, outer)| outer.span.end.saturating_sub(outer.span.start))
        .map(|(index, _)| index)
}

fn is_codegen_macro(kind: AnalyzedMacroKind) -> bool {
    matches!(
        kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineModel
    )
}

fn expansion_kind(kind: AnalyzedMacroKind) -> MacroExpansionKind {
    match kind {
        AnalyzedMacroKind::DefineEmits => MacroExpansionKind::DefineEmits,
        AnalyzedMacroKind::DefineSlots => MacroExpansionKind::DefineSlots,
        _ => MacroExpansionKind::DefineProps,
    }
}

fn resolution_failure() -> ProjectionFailure {
    if crate::request_context::current_cold_compute_completeness().is_partial() {
        partial_failure()
    } else {
        ProjectionFailure::Unresolved(UnresolvedReason::MissingDeclaration)
    }
}

fn partial_failure() -> ProjectionFailure {
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

fn macro_index(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro inventory exceeds the DTO identity space")
}

fn member_ordinal(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro member inventory exceeds the DTO identity space")
}

fn fact_footprint(finalise: FactReadSetFinalise) -> (Vec<String>, bool) {
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
