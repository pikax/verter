//! Per-SFC Vue macro codegen projection.
//!
//! This module is the TypeInfo-owned semantic producer between macro argument
//! analysis and compiler-facing DTOs. It owns no durable aggregate cache and
//! submits exactly one request-scoped scheduler cache-node job per SFC demand:
//! one invocation inventories one already-indexed SFC, reuses the canonical
//! macro payload carrier, and independently fulfills runtime and TSC demands.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroInvalidReason, MacroMemberReason,
    MacroPartialReason, MacroRuntimeBundle, MacroRuntimeEntry, MacroRuntimeOutcome,
    MacroRuntimeShape, MacroTscBundle, MacroTscEntry, MacroTscOutcome, MacroTscProjection,
    ModelRuntimeShape, OrderedRuntimeConstructors, PropsDefaultsAssociation, PropsRuntimeShape,
    RuntimeConstructor, RuntimeEmit, RuntimeProp, RuntimePropType, SynthesizedRowKind,
    TscBindingUsage, TscDeclarationFailureReason, TscDependencyDeclaration, TscEmitRow,
    TscEmitsProjection, TscInferredClassMember, TscInferredClassTypePosition, TscModelProjection,
    TscOwnerValueDependency, TscPropRow, TscPropsProjection, TscPublicPropsProjection,
    TscRetainedBinding, TscRetainedValueCarrier, TscScopeRequirements, TscScriptOwner,
    TscSemanticInferenceUnavailableReason, TscSpliceText, UnresolvedReason, UnsupportedReason,
};
use verter_semantic::analysis::component_meta::MacroExpansionKind;
use verter_semantic::analysis::{
    AnalyzedMacro, AnalyzedMacroKind, LocalDeclarationKind, MacroTypeDepUsage,
    ScriptAnalysisSnapshot,
};

use crate::locator_identity::BroadRuntimeSubjectLocator;
use crate::meta_resolve::callable_view::CallableNodeView;
use crate::meta_resolve::projectors::{build_owner_decl_identity, resolve_macro_payload};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::{FactReadSetFinalise, ResolverContext, StoreViewCompatToken};
use crate::semantic_query::{
    BroadRuntimeKind, PartialReasonSet, PathSegment, ProjectionMode, ProjectionReductionContext,
    QueryResult, ResolveDeclKey, ResultCompleteness, ScopeId, SemanticNodeData, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryValue, SurfaceProvenanceContext,
};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::VerterHost;
use verter_scheduler::cache_id::SchedulerCacheId;
use verter_scheduler::dag::PinId;
use verter_scheduler::scheduler::{ScopedCacheNodeError, ScopedCacheNodeRequest};
use verter_scheduler::stage::Priority;

/// Stable scheduler cache family for the request-local Vue macro projection.
///
/// This is an admission/singleflight identity only. The scheduler removes the
/// rendezvous at terminal state; TypeInfo's fact-keyed semantic memos remain
/// the sole durable cache authority.
const VUE_MACRO_CODEGEN_CACHE_ID: SchedulerCacheId = SchedulerCacheId(0x5655_454D_4143_5231);
const VUE_MACRO_CODEGEN_KEY_DOMAIN: &[u8] = b"verter:typeinfo:vue-macro-codegen:v1\0";

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

    const fn key_tag(self) -> u8 {
        match self {
            Self::Runtime => 0,
            Self::Tsc => 1,
            Self::RuntimeAndTsc => 2,
        }
    }
}

/// Snapshot validity lives in the scheduler input pin, never in the semantic
/// key. This keeps an SFC's key stable across edits while preventing a join
/// across resolver epochs or validation-equivalence lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VueMacroCodegenInputPin {
    pub(crate) view_epoch: u64,
    pub(crate) snapshot_pin_id: PinId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VueMacroCodegenScheduleIdentity {
    pub(crate) key_hash: [u8; 16],
    pub(crate) input_pin: VueMacroCodegenInputPin,
}

pub(crate) fn vue_macro_codegen_schedule_identity(
    ctx: &(dyn ResolverContext + Sync),
    owner_canonical: &str,
    demand: VueMacroCodegenDemand,
) -> VueMacroCodegenScheduleIdentity {
    let canonical = ctx.normalized_analysis_canonical(owner_canonical);
    vue_macro_codegen_schedule_identity_from_compat(
        canonical.as_ref(),
        demand,
        ctx.store_view().compat_token(),
    )
}

pub(crate) fn vue_macro_codegen_schedule_identity_from_compat(
    canonical: &str,
    demand: VueMacroCodegenDemand,
    compat: StoreViewCompatToken,
) -> VueMacroCodegenScheduleIdentity {
    let mut key = Vec::with_capacity(
        VUE_MACRO_CODEGEN_KEY_DOMAIN.len() + canonical.len() + std::mem::size_of::<u64>() + 3,
    );
    key.extend_from_slice(VUE_MACRO_CODEGEN_KEY_DOMAIN);
    key.extend_from_slice(&(canonical.len() as u64).to_le_bytes());
    key.extend_from_slice(canonical.as_bytes());
    key.push(demand.key_tag());
    match compat.session {
        Some(session) => {
            key.push(1);
            key.extend_from_slice(&session.to_le_bytes());
        }
        None => key.push(0),
    }

    VueMacroCodegenScheduleIdentity {
        key_hash: crate::hash::hash_16(&key),
        input_pin: VueMacroCodegenInputPin {
            view_epoch: compat.epoch,
            snapshot_pin_id: PinId(compat.validity_fingerprint),
        },
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
    /// Exactly one scheduler admission attempt for this SFC demand.
    pub scheduler_submissions: u32,
}

/// One per-call, non-retained semantic handoff for an SFC.
#[derive(Debug, Clone)]
pub(crate) struct VueMacroCodegenOutput {
    /// Content identity of the exact indexed snapshot used by the producer.
    pub origin_whole_hash: Option<verter_semantic::analysis::types::Hash16>,
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

impl VueMacroCodegenOutput {
    pub(crate) fn compiler_input(&self) -> verter_compiler::compile::VueMacroSemanticInput {
        match (&self.runtime, &self.tsc) {
            (Some(runtime), Some(tsc)) => {
                verter_compiler::compile::VueMacroSemanticInput::RuntimeAndTsc {
                    runtime: Arc::clone(runtime),
                    tsc: Arc::clone(tsc),
                }
            }
            (Some(runtime), None) => {
                verter_compiler::compile::VueMacroSemanticInput::Runtime(Arc::clone(runtime))
            }
            (None, Some(tsc)) => {
                verter_compiler::compile::VueMacroSemanticInput::Tsc(Arc::clone(tsc))
            }
            (None, None) => verter_compiler::compile::VueMacroSemanticInput::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ProjectionFailure {
    Partial(MacroPartialReason),
    Unresolved(UnresolvedReason),
    Unsupported(UnsupportedReason),
    Invalid(MacroInvalidReason),
}

struct TscScopeInventory<'a> {
    analysis: &'a ScriptAnalysisSnapshot,
    shallow_state: &'a crate::resolver_core::ShallowFileState,
    raw_source: &'a str,
}

#[derive(Debug, Clone, Copy)]
enum ClassInferenceFailure {
    InferenceUnavailable(verter_type_expr::facts::InferenceUnavailableReason),
    Unsupported(UnsupportedReason),
    Unresolved(UnresolvedReason),
}

impl ClassInferenceFailure {
    fn declaration_reason(self) -> TscDeclarationFailureReason {
        match self {
            Self::InferenceUnavailable(reason) => {
                crate::request_context::mark_request_result_inference_budget_exceeded();
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::InferenceBudgetExceeded,
                );
                TscDeclarationFailureReason::SemanticInferenceUnavailable(match reason {
                    verter_type_expr::facts::InferenceUnavailableReason::DepthBudgetExceeded => {
                        TscSemanticInferenceUnavailableReason::DepthBudgetExceeded
                    }
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded => {
                        TscSemanticInferenceUnavailableReason::WorkBudgetExceeded
                    }
                })
            }
            Self::Unsupported(reason) => TscDeclarationFailureReason::Unsupported(reason),
            Self::Unresolved(reason) => TscDeclarationFailureReason::Unresolved(reason),
        }
    }
}

fn tsc_script_owner(
    owner: verter_type_expr::TopLevelOwnerId,
) -> Result<TscScriptOwner, ProjectionFailure> {
    match (owner.kind(), owner.ordinal()) {
        (verter_type_expr::TopLevelOwnerKind::Instance, 0) => Ok(TscScriptOwner::Setup),
        (verter_type_expr::TopLevelOwnerKind::Module, 0) => Ok(TscScriptOwner::Companion),
        _ => Err(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        )),
    }
}

fn top_level_owner(owner: TscScriptOwner) -> verter_type_expr::TopLevelOwnerId {
    match owner {
        TscScriptOwner::Setup => verter_type_expr::TopLevelOwnerId::instance(0),
        TscScriptOwner::Companion => verter_type_expr::TopLevelOwnerId::module(0),
    }
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
            Self::Invalid(reason) => MacroRuntimeOutcome::Invalid(MacroFailure::new(reason, None)),
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
            Self::Invalid(reason) => MacroTscOutcome::Invalid(MacroFailure::new(reason, None)),
        }
    }

    fn member(self) -> MacroMemberReason {
        match self {
            Self::Partial(reason) => MacroMemberReason::Partial(reason),
            Self::Unresolved(reason) => MacroMemberReason::Unresolved(reason),
            Self::Unsupported(reason) => MacroMemberReason::Unsupported(reason),
            Self::Invalid(_) => {
                MacroMemberReason::Unsupported(UnsupportedReason::SemanticConstruct)
            }
        }
    }
}

struct ProducerState {
    origin_whole_hash: Option<verter_semantic::analysis::types::Hash16>,
    runtime_entries: Vec<MacroRuntimeEntry>,
    tsc_entries: Vec<MacroTscEntry>,
    counters: VueMacroCodegenCounters,
    completeness: ResultCompleteness,
}

fn cancelled_vue_macro_codegen_output(
    ctx: &(dyn ResolverContext + Sync),
    owner_canonical: &str,
    demand: VueMacroCodegenDemand,
) -> VueMacroCodegenOutput {
    terminal_partial_vue_macro_codegen_output(
        ctx,
        owner_canonical,
        demand,
        PartialReasonSet::CANCELLED,
        MacroPartialReason::Cancelled,
    )
}

/// Build the ReturnOnly handoff for a terminal scheduler failure without
/// entering semantic classification. Inventory reads are permitted so each
/// demanded macro retains its stable identity and typed `Partial` outcome;
/// the result is explicitly non-cacheable and is never published by the
/// request-scoped scheduler rendezvous.
fn terminal_partial_vue_macro_codegen_output(
    ctx: &(dyn ResolverContext + Sync),
    owner_canonical: &str,
    demand: VueMacroCodegenDemand,
    completeness_reason: PartialReasonSet,
    macro_reason: MacroPartialReason,
) -> VueMacroCodegenOutput {
    let completeness = ResultCompleteness::partial(completeness_reason);
    crate::request_context::fold_result_completeness(completeness);

    let indexed = ctx.indexed_for_current_content(owner_canonical);
    let base_source = (indexed.is_none() && ctx.active_session_view().is_none())
        .then(|| {
            ctx.host_for_fact_tracer_install()
                .scheduler_source(owner_canonical)
        })
        .flatten();
    let origin_whole_hash = indexed
        .as_ref()
        .map(|indexed| indexed.whole_hash)
        .or_else(|| base_source.as_ref().map(|source| source.whole_hash));
    let script_analysis = indexed
        .as_ref()
        .and_then(|indexed| indexed.script_analysis.as_ref().map(Arc::clone))
        .or_else(|| {
            base_source
                .as_ref()
                .and_then(|source| source.downcast_data::<crate::host_executor::HostSourceData>())
                .map(|data| Arc::clone(&data.parse.script_analysis))
        });
    let mut runtime_entries = Vec::new();
    let mut tsc_entries = Vec::new();
    if let Some(macros) = script_analysis
        .as_ref()
        .map(|analysis| analysis.macros.as_slice())
    {
        for (payload_index, mac) in macros.iter().enumerate() {
            if mac.kind == AnalyzedMacroKind::WithDefaults || !mac.is_type_based {
                continue;
            }
            let defaults_index = (mac.kind == AnalyzedMacroKind::DefineProps)
                .then(|| containing_with_defaults_index(macros, payload_index))
                .flatten();
            let effective_index = defaults_index.unwrap_or(payload_index);
            let syntax_index = top_level_syntax_index(macros, effective_index);
            let macro_index = macro_index(effective_index);

            if demand.wants_runtime() && is_codegen_macro(mac.kind) {
                runtime_entries.push(MacroRuntimeEntry {
                    syntax_index,
                    macro_index,
                    outcome: MacroRuntimeOutcome::Partial(MacroFailure::new(macro_reason, None)),
                });
            }
            if demand.wants_tsc() && is_codegen_macro(mac.kind) {
                tsc_entries.push(MacroTscEntry {
                    syntax_index,
                    macro_index,
                    outcome: MacroTscOutcome::Partial(MacroFailure::new(macro_reason, None)),
                });
            }
        }
    }

    VueMacroCodegenOutput {
        origin_whole_hash,
        runtime: demand.wants_runtime().then(|| {
            Arc::new(MacroRuntimeBundle {
                entries: runtime_entries,
            })
        }),
        tsc: demand.wants_tsc().then(|| {
            Arc::new(MacroTscBundle {
                entries: tsc_entries,
            })
        }),
        transitive_canonicals: Vec::new(),
        completeness,
        facts_cacheable: false,
        counters: VueMacroCodegenCounters {
            producer_invocations: 1,
            scheduler_submissions: 1,
            ..VueMacroCodegenCounters::default()
        },
    }
}

impl VerterHost {
    /// Produce a request-local bundle from one coherent cold-seed view.
    pub(crate) fn produce_vue_macro_codegen(
        &self,
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> VueMacroCodegenOutput {
        let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &cold_seed, overlay);
        self.produce_vue_macro_codegen_with_ctx(&ctx, owner_canonical, demand)
    }

    /// Produce the requested Vue macro codegen bundle from one request-bound
    /// resolver context.
    ///
    /// The result is intentionally not retained as an aggregate graph-id
    /// cache. Underlying TypeInfo semantic queries retain their own canonical
    /// memo entries and singleflight behavior.
    pub(crate) fn produce_vue_macro_codegen_with_ctx(
        &self,
        ctx: &(dyn ResolverContext + Sync),
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> VueMacroCodegenOutput {
        let identity = vue_macro_codegen_schedule_identity(ctx, owner_canonical, demand);
        let request = ScopedCacheNodeRequest {
            cache_id: VUE_MACRO_CODEGEN_CACHE_ID,
            key_hash: identity.key_hash,
            view_epoch: identity.input_pin.view_epoch,
            snapshot_pin_id: identity.input_pin.snapshot_pin_id,
            priority: Priority::Interactive,
            request_context: verter_scheduler::request_context::current_context(),
        };

        match self
            .scheduler()
            .execute_scoped_cache_node(request, |job_cancellation| {
                if job_cancellation.is_cancelled() {
                    crate::request_context::mark_request_result_cancelled();
                    return cancelled_vue_macro_codegen_output(ctx, owner_canonical, demand);
                }
                self.compute_vue_macro_codegen_output(ctx, owner_canonical, demand)
            }) {
            Ok(output) => output.as_ref().clone(),
            Err(ScopedCacheNodeError::Cancelled) => {
                crate::request_context::mark_request_result_cancelled();
                cancelled_vue_macro_codegen_output(ctx, owner_canonical, demand)
            }
            Err(ScopedCacheNodeError::Shutdown) => terminal_partial_vue_macro_codegen_output(
                ctx,
                owner_canonical,
                demand,
                PartialReasonSet::UNSTABLE_STATE,
                MacroPartialReason::UnstableState,
            ),
            Err(
                ScopedCacheNodeError::Panicked
                | ScopedCacheNodeError::TypeMismatch
                | ScopedCacheNodeError::Reentrant,
            ) => terminal_partial_vue_macro_codegen_output(
                ctx,
                owner_canonical,
                demand,
                PartialReasonSet::SEMANTIC_QUERY_FAULT,
                MacroPartialReason::IncompleteTraversal,
            ),
        }
    }

    fn compute_vue_macro_codegen_output(
        &self,
        ctx: &(dyn ResolverContext + Sync),
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> VueMacroCodegenOutput {
        #[cfg(test)]
        if let Some(rendezvous) = self
            .test_force
            .vue_macro_codegen_build_rendezvous
            .lock()
            .clone()
        {
            rendezvous.0.wait();
            rendezvous.1.wait();
        }
        let (mut state, finalise) =
            crate::fact_signature_helpers::install_fact_tracer(self, || {
                let _completeness_scope =
                    crate::request_context::ColdComputeCompletenessScope::enter();
                let mut state = self.produce_vue_macro_codegen_inner(ctx, owner_canonical, demand);
                state.completeness = crate::request_context::current_cold_compute_completeness();
                state
            });
        state.counters.scheduler_submissions = 1;

        let (transitive_canonicals, facts_cacheable) = fact_footprint(finalise);
        VueMacroCodegenOutput {
            origin_whole_hash: state.origin_whole_hash,
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
        ctx: &(dyn ResolverContext + Sync),
        owner_canonical: &str,
        demand: VueMacroCodegenDemand,
    ) -> ProducerState {
        let mut state = ProducerState {
            origin_whole_hash: None,
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
        state.origin_whole_hash = Some(serve.indexed.whole_hash);
        let Some(script_analysis) = serve.indexed.script_analysis.as_ref() else {
            return state;
        };
        let macros = &script_analysis.macros;
        let tsc_scope_inventory = TscScopeInventory {
            analysis: script_analysis,
            shallow_state: &serve.indexed.shallow_state,
            raw_source: &serve.indexed.raw_source,
        };
        let dispatch = ProjectSemanticDispatch::new(ctx);

        for (payload_index, mac) in macros.iter().enumerate() {
            if mac.kind == AnalyzedMacroKind::WithDefaults || !mac.is_type_based {
                continue;
            }
            let owner = build_owner_decl_identity(ctx, owner_canonical, mac.owner);

            let defaults_index = (mac.kind == AnalyzedMacroKind::DefineProps)
                .then(|| containing_with_defaults_index(macros, payload_index))
                .flatten();
            let effective_index = defaults_index.unwrap_or(payload_index);
            let syntax_index = top_level_syntax_index(macros, effective_index);
            let macro_index = macro_index(effective_index);

            if !is_codegen_macro(mac.kind) {
                continue;
            }

            if mac.parsed_type_argument.is_none() {
                let failure = ProjectionFailure::Unresolved(UnresolvedReason::MissingTypeArgument);
                if demand.wants_runtime() {
                    state.runtime_entries.push(MacroRuntimeEntry {
                        syntax_index,
                        macro_index,
                        outcome: failure.runtime(),
                    });
                }
                if demand.wants_tsc() {
                    state.tsc_entries.push(MacroTscEntry {
                        syntax_index,
                        macro_index,
                        outcome: failure.tsc(),
                    });
                }
                continue;
            }

            // Each macro owns one completeness scope, and runtime/TSC are
            // independently nested below it. A partial demand must taint the
            // file-level result and cacheability without changing a sibling
            // macro's typed outcome or the other demand's outcome.
            let macro_scope = crate::request_context::ColdComputeCompletenessScope::enter();
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
            let payload_failure =
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    Some(partial_failure())
                } else if payload.is_none() {
                    Some(resolution_failure())
                } else {
                    None
                };

            if demand.wants_runtime() {
                let outcome = {
                    let _runtime_scope =
                        crate::request_context::ColdComputeCompletenessScope::enter();
                    match (payload_failure, payload) {
                        (Some(failure), _) => failure.runtime(),
                        (None, Some(payload)) => match mac.kind {
                            AnalyzedMacroKind::DefineProps => {
                                match dispatch
                                    .broad_runtime_subject_for_macro(&owner, payload_index)
                                {
                                    Some(subject) => self.project_runtime_props(
                                        ctx,
                                        &dispatch,
                                        payload,
                                        &subject,
                                        script_analysis,
                                        mac,
                                        payload_index,
                                        defaults_index,
                                        &mut state.counters,
                                    ),
                                    None => ProjectionFailure::Unsupported(
                                        UnsupportedReason::SemanticConstruct,
                                    )
                                    .runtime(),
                                }
                            }
                            AnalyzedMacroKind::DefineEmits => self.project_runtime_emits(
                                ctx,
                                &dispatch,
                                payload,
                                mac,
                                payload_index,
                                effective_index,
                                &mut state.counters,
                            ),
                            AnalyzedMacroKind::DefineModel => {
                                match dispatch
                                    .broad_runtime_subject_for_macro(&owner, payload_index)
                                {
                                    Some(subject) => self.project_runtime_model(
                                        &dispatch,
                                        subject,
                                        mac,
                                        effective_index,
                                        &mut state.counters,
                                    ),
                                    None => ProjectionFailure::Unsupported(
                                        UnsupportedReason::SemanticConstruct,
                                    )
                                    .runtime(),
                                }
                            }
                            _ => unreachable!("codegen macro filter is exhaustive"),
                        },
                        (None, None) => unreachable!("payload failure covers an absent payload"),
                    }
                };
                state.runtime_entries.push(MacroRuntimeEntry {
                    syntax_index,
                    macro_index,
                    outcome,
                });
            }

            if demand.wants_tsc() {
                let outcome = {
                    let _tsc_scope = crate::request_context::ColdComputeCompletenessScope::enter();
                    match (payload_failure, payload) {
                        (Some(failure), _) => failure.tsc(),
                        (None, Some(payload)) => self.project_tsc_macro(
                            ctx,
                            &dispatch,
                            payload,
                            mac,
                            payload_index,
                            effective_index,
                            &tsc_scope_inventory,
                            &mut state.counters,
                        ),
                        (None, None) => unreachable!("payload failure covers an absent payload"),
                    }
                };
                state.tsc_entries.push(MacroTscEntry {
                    syntax_index,
                    macro_index,
                    outcome,
                });
            }

            let macro_completeness = crate::request_context::current_cold_compute_completeness();
            macro_scope.discard();
            crate::request_context::fold_result_completeness(macro_completeness);
        }

        state
    }

    #[allow(clippy::too_many_arguments)]
    fn project_tsc_macro(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        mac: &AnalyzedMacro,
        payload_index: usize,
        effective_index: usize,
        scope_inventory: &TscScopeInventory<'_>,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroTscOutcome {
        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                if probe_definitely_non_object_root(dispatch, payload) {
                    return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).tsc();
                }
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
                    return partial_failure().tsc();
                }
                let Some(surface) = surface else {
                    return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).tsc();
                };

                let mut testing_rows = Vec::new();
                for member in surface
                    .members
                    .iter()
                    .filter(|member| member.visibility.is_public())
                {
                    let type_text = match render_tsc_node(ctx, member.value, counters) {
                        Ok(text) => text,
                        Err(failure) => return failure.tsc(),
                    };
                    testing_rows.push(TscPropRow {
                        name: member.name.as_ref().to_owned(),
                        optional: member.optional,
                        type_text,
                        anchor: member_anchor(mac, payload_index, member.name.as_ref()),
                    });
                }
                let scope = match tsc_scope_requirements(mac, scope_inventory) {
                    Ok(scope) => scope,
                    Err(failure) => return failure.tsc(),
                };
                MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
                    public: TscPublicPropsProjection::AuthoredArgument {
                        anchor: MacroAnchor::MacroArgument {
                            macro_index: macro_index(payload_index),
                        },
                    },
                    testing_rows,
                    scope,
                }))
            }
            AnalyzedMacroKind::DefineEmits => {
                if probe_definitely_non_object_root(dispatch, payload) {
                    return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).tsc();
                }
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
                    return partial_failure().tsc();
                }
                let Some(surface) = surface else {
                    return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).tsc();
                };
                let events = match tsc_emit_rows(
                    ctx,
                    dispatch,
                    &surface,
                    mac,
                    payload_index,
                    effective_index,
                    counters,
                ) {
                    Ok(events) => events,
                    Err(failure) => return failure.tsc(),
                };
                let scope = match tsc_scope_requirements(mac, scope_inventory) {
                    Ok(scope) => scope,
                    Err(failure) => return failure.tsc(),
                };
                MacroTscOutcome::Complete(MacroTscProjection::Emits(TscEmitsProjection {
                    events,
                    scope,
                }))
            }
            AnalyzedMacroKind::DefineModel => {
                let value_type = match render_tsc_node(ctx, payload, counters) {
                    Ok(text) => text,
                    Err(failure) => return failure.tsc(),
                };
                let name = mac.model_name.as_deref().unwrap_or("modelValue").to_owned();
                let optional = mac
                    .prop_fields
                    .first()
                    .is_none_or(|field| field.is_optional);
                let scope = match tsc_scope_requirements(mac, scope_inventory) {
                    Ok(scope) => scope,
                    Err(failure) => return failure.tsc(),
                };
                MacroTscOutcome::Complete(MacroTscProjection::Model(TscModelProjection {
                    name,
                    optional,
                    value_type,
                    anchor: MacroAnchor::Synthesized {
                        macro_index: macro_index(effective_index),
                        row: SynthesizedRowKind::ModelProp,
                    },
                    scope,
                }))
            }
            _ => unreachable!("codegen macro filter is exhaustive"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn project_runtime_props(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        runtime_subject: &BroadRuntimeSubjectLocator,
        analysis: &ScriptAnalysisSnapshot,
        mac: &AnalyzedMacro,
        payload_index: usize,
        defaults_index: Option<usize>,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        if probe_definitely_non_object_root(dispatch, payload) {
            return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).runtime();
        }
        counters.root_shallow_demands += 1;
        let surface = self.project_shallow_surface_graph_only(
            ctx,
            dispatch,
            payload,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::vue_runtime_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
            ),
            None,
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        let Some(surface) = surface else {
            return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).runtime();
        };

        let member_dependency_names = analysis
            .macro_type_deps
            .iter()
            .filter(|dependency| {
                dependency.macro_index == payload_index && dependency.macro_span == mac.span
            })
            .filter(|dependency| {
                matches!(
                    dependency.usage,
                    MacroTypeDepUsage::Member | MacroTypeDepUsage::ValueQueryMember
                )
            })
            .map(|dependency| dependency.type_name.as_str())
            .collect::<FxHashSet<_>>();

        let mut props = Vec::new();
        for member in surface
            .members
            .iter()
            .filter(|member| member.visibility.is_public())
        {
            let type_shape = if direct_member_dependency_is_missing(
                dispatch,
                member.value,
                &member_dependency_names,
            ) {
                RuntimePropType::Degraded(MacroFailure::new(
                    MacroMemberReason::Unresolved(UnresolvedReason::MissingDependency),
                    None,
                ))
            } else {
                match classify_runtime(
                    dispatch,
                    runtime_subject.member(Arc::clone(&member.name)),
                    counters,
                ) {
                    Ok(classification) => RuntimePropType::Resolved {
                        constructors: classification.constructors,
                        skip_check: classification.skip_check,
                    },
                    Err(failure) => {
                        RuntimePropType::Degraded(MacroFailure::new(failure.member(), None))
                    }
                }
            };
            props.push(RuntimeProp {
                name: member.name.as_ref().to_owned(),
                optional: member.optional,
                type_shape,
                anchor: member_anchor(mac, payload_index, member.name.as_ref()),
            });
        }

        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
            defaults: defaults_association(payload_index, defaults_index),
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
        if probe_definitely_non_object_root(dispatch, payload) {
            return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).runtime();
        }
        counters.root_shallow_demands += 1;
        let runtime_context = ProjectionReductionContext::vue_runtime_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        );
        let surface = self.project_shallow_surface_graph_only(
            ctx,
            dispatch,
            payload,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            runtime_context,
            None,
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        let Some(surface) = surface else {
            return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).runtime();
        };

        let emits = emit_rows(
            dispatch,
            &surface,
            mac,
            payload_index,
            effective_index,
            runtime_context,
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(emits))
    }

    fn project_runtime_model(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        runtime_subject: BroadRuntimeSubjectLocator,
        mac: &AnalyzedMacro,
        effective_index: usize,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        let classification = match classify_runtime(dispatch, runtime_subject, counters) {
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
                type_shape: RuntimePropType::Resolved {
                    constructors: classification.constructors,
                    skip_check: classification.skip_check,
                },
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
                type_shape: RuntimePropType::Resolved {
                    constructors: OrderedRuntimeConstructors::default(),
                    skip_check: false,
                },
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelModifiersProp,
                },
            },
        }))
    }
}

fn render_tsc_node(
    ctx: &dyn ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    counters.tsc_materializations += 1;
    let rendered = crate::typeinfo::raise::render_node_display_with_ctx(ctx, node);
    if crate::request_context::current_cold_compute_completeness().is_partial() {
        return Err(partial_failure());
    }
    rendered
        .map(TscSpliceText::new)
        .ok_or(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        ))
}

fn tsc_scope_requirements(
    mac: &AnalyzedMacro,
    inventory: &TscScopeInventory<'_>,
) -> Result<TscScopeRequirements, ProjectionFailure> {
    let macro_owner = tsc_script_owner(mac.owner)?;
    let mut required_imports = BTreeMap::new();
    let mut roots = Vec::new();
    for name in &mac.type_references {
        match visible_type_binding(inventory, macro_owner, name)? {
            Some(VisibleTypeBinding::Import(owner)) => {
                required_imports.insert((owner, name.clone()), TscBindingUsage::TypePosition);
            }
            Some(VisibleTypeBinding::Local(owner)) => roots.push((owner, name.clone())),
            None => {}
        }
    }
    for dependency in inventory
        .analysis
        .macro_type_deps
        .iter()
        .filter(|dependency| dependency.macro_span == mac.span)
        .filter(|dependency| dependency.usage.is_value_query())
    {
        let Some(owner) = visible_import_owner(
            inventory,
            macro_owner,
            &dependency.type_name,
            Some(&dependency.import_source),
        )?
        else {
            continue;
        };
        if let Some(usage) = required_imports.get_mut(&(owner, dependency.type_name.clone())) {
            *usage = TscBindingUsage::ValueQuery;
        }
    }
    let mut direct_owner_value_dependencies = Vec::new();
    for name in &mac.type_references {
        if visible_import_owner(inventory, macro_owner, name, None)?.is_some() {
            continue;
        }
        let Ok(owner) = local_value_owner(inventory, macro_owner, name) else {
            continue;
        };
        let declaration_owner = top_level_owner(owner);
        if !inventory
            .shallow_state
            .has_value_symbol_in(declaration_owner, name)
            || inventory
                .shallow_state
                .has_type_symbol_in(declaration_owner, name)
        {
            continue;
        }
        direct_owner_value_dependencies.push(TscOwnerValueDependency {
            owner,
            name: name.clone(),
        });
    }
    direct_owner_value_dependencies.sort_by(|left, right| {
        (left.owner, left.name.as_str()).cmp(&(right.owner, right.name.as_str()))
    });
    direct_owner_value_dependencies.dedup();

    roots.sort_by_key(|(owner, name)| declaration_order(inventory, *owner, name));
    roots.dedup();

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut owner_value_dependencies =
        BTreeMap::<(TscScriptOwner, String), BTreeSet<TscOwnerValueDependency>>::new();
    let mut retained_value_dependencies =
        BTreeMap::<(TscScriptOwner, String), BTreeSet<TscRetainedValueCarrier>>::new();
    let mut declaration_ordered_names = Vec::new();
    for (owner, root) in roots {
        collect_local_declaration_closure(
            owner,
            &root,
            inventory,
            &mut required_imports,
            &mut visiting,
            &mut visited,
            &mut owner_value_dependencies,
            &mut retained_value_dependencies,
            &mut declaration_ordered_names,
        )?;
    }

    let retained_value_carriers = declaration_ordered_names
        .iter()
        .filter_map(|(owner, name)| retained_value_carrier(inventory, *owner, name).ok())
        .map(|carrier| ((carrier.owner, carrier.name.clone()), carrier))
        .collect::<BTreeMap<_, _>>();

    let mut dependency_declarations = Vec::new();
    for (owner, name) in declaration_ordered_names {
        let mut found = false;
        for (contributor_ordinal, entry) in inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| {
                entry.name == name
                    && entry.owner == top_level_owner(owner)
                    && matches!(
                        entry.kind,
                        LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                    )
            })
            .enumerate()
        {
            inventory
                .raw_source
                .get(entry.span.start as usize..entry.span.end as usize)
                .ok_or(ProjectionFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ))?;
            found = true;
            let (inferred_class_members, inferred_value_dependencies, declaration_failure) =
                match inferred_class_members(
                    top_level_owner(owner),
                    &name,
                    contributor_ordinal,
                    entry.span,
                    entry.kind == LocalDeclarationKind::TypeAndValue,
                    inventory,
                ) {
                    Ok(inferred) => (inferred.members, inferred.value_dependencies, None),
                    Err(failure) => (
                        Vec::new(),
                        BTreeSet::new(),
                        Some(failure.declaration_reason()),
                    ),
                };
            for dependency in inferred_value_dependencies {
                let root = dependency.root();
                if let Some(import_owner) = visible_import_owner(inventory, owner, root, None)? {
                    let import_key = (import_owner, root.to_string());
                    merge_required_import(
                        &mut required_imports,
                        import_key,
                        TscBindingUsage::ValueQuery,
                    );
                } else {
                    let value_owner = local_value_owner(inventory, owner, root)?;
                    if let Some(carrier) =
                        retained_value_carriers.get(&(value_owner, root.to_string()))
                    {
                        retained_value_dependencies
                            .entry((owner, name.clone()))
                            .or_default()
                            .insert(carrier.clone());
                        continue;
                    }
                    owner_value_dependencies
                        .entry((owner, name.clone()))
                        .or_default()
                        .insert(TscOwnerValueDependency {
                            owner: value_owner,
                            name: root.to_string(),
                        });
                }
            }
            dependency_declarations.push(TscDependencyDeclaration {
                owner,
                name: name.clone(),
                contributor_ordinal: u32::try_from(contributor_ordinal).map_err(|_| {
                    ProjectionFailure::Unresolved(UnresolvedReason::MissingDependency)
                })?,
                owner_value_dependencies: owner_value_dependencies
                    .get(&(owner, name.clone()))
                    .map(|dependencies| dependencies.iter().cloned().collect())
                    .unwrap_or_default(),
                retained_value_carriers: retained_value_dependencies
                    .get(&(owner, name.clone()))
                    .map(|dependencies| dependencies.iter().cloned().collect())
                    .unwrap_or_default(),
                declaration_failure,
                inferred_class_members,
            });
        }
        if !found {
            return Err(ProjectionFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
    }

    let mut retained_names = BTreeSet::new();
    let mut retained_bindings = Vec::new();
    for import in &inventory.analysis.imports {
        let owner = tsc_script_owner(import.owner)?;
        for binding in &import.bindings {
            let key = (owner, binding.name.clone());
            let Some(usage) = required_imports.get(&key).copied() else {
                continue;
            };
            if retained_names.insert(key) {
                retained_bindings.push(TscRetainedBinding {
                    owner,
                    local_name: binding.name.clone(),
                    usage,
                });
            }
        }
    }
    if retained_bindings.len() != required_imports.len() {
        return Err(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    }

    Ok(TscScopeRequirements {
        owner_value_dependencies: direct_owner_value_dependencies,
        retained_bindings,
        dependency_declarations,
    })
}

fn local_value_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<TscScriptOwner, ProjectionFailure> {
    visible_owner(
        requester,
        inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| entry.name == name)
            .filter(|entry| {
                matches!(
                    entry.kind,
                    LocalDeclarationKind::Value | LocalDeclarationKind::TypeAndValue
                )
            })
            .map(|entry| tsc_script_owner(entry.owner))
            .collect::<Result<BTreeSet<_>, _>>()?,
    )
    .ok_or(ProjectionFailure::Unresolved(
        UnresolvedReason::MissingDependency,
    ))
}

fn declaration_order(
    inventory: &TscScopeInventory<'_>,
    owner: TscScriptOwner,
    name: &str,
) -> (u32, TscScriptOwner, String) {
    (
        inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| entry.name == name && entry.owner == top_level_owner(owner))
            .map(|entry| entry.span.start)
            .min()
            .unwrap_or(u32::MAX),
        owner,
        name.to_owned(),
    )
}

#[derive(Clone, Copy)]
enum VisibleTypeBinding {
    Import(TscScriptOwner),
    Local(TscScriptOwner),
}

fn visible_type_binding(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<Option<VisibleTypeBinding>, ProjectionFailure> {
    for owner in visible_owner_order(requester) {
        let has_import = visible_import_owner(inventory, owner, name, None)? == Some(owner);
        let has_local = inventory.analysis.declaration_entries.iter().any(|entry| {
            entry.name == name
                && entry.owner == top_level_owner(owner)
                && matches!(
                    entry.kind,
                    LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                )
        });
        match (has_import, has_local) {
            (true, false) => return Ok(Some(VisibleTypeBinding::Import(owner))),
            (false, true) => return Ok(Some(VisibleTypeBinding::Local(owner))),
            (true, true) => {
                return Err(ProjectionFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            }
            (false, false) => {}
        }
    }
    Ok(None)
}

fn visible_local_type_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<TscScriptOwner, ProjectionFailure> {
    match visible_type_binding(inventory, requester, name)? {
        Some(VisibleTypeBinding::Local(owner)) => Ok(owner),
        Some(VisibleTypeBinding::Import(_)) => Err(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        )),
        None => Err(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        )),
    }
}

fn visible_import_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
    source: Option<&str>,
) -> Result<Option<TscScriptOwner>, ProjectionFailure> {
    let mut owners = BTreeSet::new();
    for import in &inventory.analysis.imports {
        if source.is_none_or(|source| import.source == source)
            && import.bindings.iter().any(|binding| binding.name == name)
        {
            owners.insert(tsc_script_owner(import.owner)?);
        }
    }
    Ok(visible_owner(requester, owners))
}

fn visible_owner(
    requester: TscScriptOwner,
    owners: BTreeSet<TscScriptOwner>,
) -> Option<TscScriptOwner> {
    visible_owner_order(requester)
        .into_iter()
        .find(|owner| owners.contains(owner))
}

fn visible_owner_order(requester: TscScriptOwner) -> Vec<TscScriptOwner> {
    match requester {
        TscScriptOwner::Setup => vec![TscScriptOwner::Setup, TscScriptOwner::Companion],
        TscScriptOwner::Companion => vec![TscScriptOwner::Companion],
    }
}

struct InferredClassProjection {
    members: Vec<TscInferredClassMember>,
    value_dependencies: BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
}

fn inferred_class_members(
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
    contributor_ordinal: usize,
    declaration_span: verter_span::Span,
    include_static: bool,
    inventory: &TscScopeInventory<'_>,
) -> Result<InferredClassProjection, ClassInferenceFailure> {
    fn collect_overload_groups(
        ty: &verter_type_expr::TypeExpr,
        is_static: bool,
        groups: &mut BTreeSet<(String, bool)>,
    ) -> Result<(), verter_type_expr::facts::InferenceUnavailableReason> {
        use crate::resolver_core::shallow_file_state::SEMANTIC_INFERENCE_TRAVERSAL_BUDGET;

        let mut pending = vec![ty];
        let mut visited = 0usize;
        while let Some(current) = pending.pop() {
            visited = visited.saturating_add(1);
            if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
                return Err(
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                );
            }
            match current {
                verter_type_expr::TypeExpr::Object(object) => {
                    for member in &object.properties {
                        if let verter_type_expr::ObjectMember::Method(method) = member {
                            if !method.has_implementation_body {
                                groups.insert((method.name.clone(), is_static));
                            }
                        }
                    }
                }
                verter_type_expr::TypeExpr::Intersection(parts)
                | verter_type_expr::TypeExpr::Union(parts) => pending.extend(parts.iter()),
                verter_type_expr::TypeExpr::Parenthesized(inner) => pending.push(inner),
                _ => {}
            }
        }
        Ok(())
    }

    let Some(lowered) = inventory.shallow_state.effective_type_decl_in(owner, name) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };
    if lowered.kind != verter_semantic::analysis::type_eval::TypeDeclKind::Class {
        return Ok(InferredClassProjection {
            members: Vec::new(),
            value_dependencies: BTreeSet::new(),
        });
    }
    let contributors = match inventory
        .shallow_state
        .decl_bodies()
        .transient_type_bodies_in(owner, name)
    {
        crate::decl_body_memo::DemandOutcome::Ready(Some(contributors)) => contributors,
        _ => {
            return Err(ClassInferenceFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
    };
    let Some(body) = contributors.get(contributor_ordinal) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };
    let Some(contributor_fact) = lowered.contributor_facts.get(contributor_ordinal) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };

    struct Candidate<'a> {
        start: u32,
        name: &'a str,
        is_static: bool,
        position: TscInferredClassTypePosition,
        ty: Option<&'a verter_type_expr::TypeExpr>,
        has_implementation_body: bool,
        return_inference: Option<verter_type_expr::facts::ReturnInferenceCompleteness>,
    }

    let mut overload_groups = BTreeSet::new();
    collect_overload_groups(body, false, &mut overload_groups)
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
    let mut candidates = Vec::new();
    let mut pending = vec![body];
    while let Some(current) = pending.pop() {
        match current {
            verter_type_expr::TypeExpr::Object(object) => {
                for (member_index, member) in object.properties.iter().enumerate() {
                    match member {
                        verter_type_expr::ObjectMember::Property(property)
                            if property.spans.type_annotation.is_none() =>
                        {
                            if let Some(span) = property.spans.declaration.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }) {
                                candidates.push(Candidate {
                                    start: span.start,
                                    name: &property.name,
                                    is_static: false,
                                    position: TscInferredClassTypePosition::Property,
                                    ty: Some(&property.ty),
                                    has_implementation_body: false,
                                    return_inference: None,
                                });
                            }
                        }
                        verter_type_expr::ObjectMember::Method(method) => {
                            for parameter in &method.function.parameters {
                                if parameter.has_ts_annotation || parameter.is_parameter_property {
                                    continue;
                                }
                                if let (Some(name), Some(span)) = (
                                    parameter.name.as_deref(),
                                    parameter.span.filter(|span| {
                                        span.start >= declaration_span.start
                                            && span.end <= declaration_span.end
                                    }),
                                ) {
                                    candidates.push(Candidate {
                                        start: span.start,
                                        name,
                                        is_static: false,
                                        position: TscInferredClassTypePosition::Parameter,
                                        ty: Some(&parameter.ty),
                                        has_implementation_body: method.has_implementation_body,
                                        return_inference: None,
                                    });
                                }
                            }
                            if method.function.spans.return_type.is_none()
                                && method.method_kind != verter_type_expr::ObjectMethodKind::Set
                            {
                                if let Some(span) = method.spans.declaration.filter(|span| {
                                    span.start >= declaration_span.start
                                        && span.end <= declaration_span.end
                                }) {
                                    candidates.push(Candidate {
                                        start: span.start,
                                        name: &method.name,
                                        is_static: false,
                                        position: TscInferredClassTypePosition::Return,
                                        ty: method.function.return_type.as_deref(),
                                        has_implementation_body: method.has_implementation_body,
                                        return_inference: u32::try_from(member_index)
                                            .ok()
                                            .and_then(|member_ordinal| {
                                                let member_path = [member_ordinal];
                                                contributor_fact
                                                    .return_inference_for_member_path(&member_path)
                                            }),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            verter_type_expr::TypeExpr::Intersection(parts)
            | verter_type_expr::TypeExpr::Union(parts) => {
                pending.extend(parts.iter().rev());
            }
            verter_type_expr::TypeExpr::Parenthesized(inner) => pending.push(inner),
            _ => {}
        }
    }

    let static_decl = if include_static {
        Some(
            inventory
                .shallow_state
                .effective_value_decl_in(owner, name)
                .ok_or(ClassInferenceFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ))?,
        )
    } else {
        None
    };
    let static_parts = if include_static {
        match inventory
            .shallow_state
            .decl_bodies()
            .transient_value_parts_in(owner, name)
        {
            crate::decl_body_memo::DemandOutcome::Ready(Some(parts)) => Some(parts),
            _ => {
                return Err(ClassInferenceFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ));
            }
        }
    } else {
        None
    };
    if let Some(object) = static_parts
        .as_ref()
        .and_then(|parts| parts.object_shape.as_ref())
    {
        let Some(object_fact) = static_decl
            .as_ref()
            .and_then(|declaration| declaration.object_shape.as_ref())
        else {
            return Err(ClassInferenceFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        };
        collect_overload_groups(
            &verter_type_expr::TypeExpr::Object(object.clone().into()),
            true,
            &mut overload_groups,
        )
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
        for (member_index, member) in object.properties.iter().enumerate() {
            match member {
                verter_type_expr::ObjectMember::Property(property)
                    if property.spans.type_annotation.is_none() =>
                {
                    if let Some(span) = property.spans.declaration.filter(|span| {
                        span.start >= declaration_span.start && span.end <= declaration_span.end
                    }) {
                        candidates.push(Candidate {
                            start: span.start,
                            name: &property.name,
                            is_static: true,
                            position: TscInferredClassTypePosition::Property,
                            ty: Some(&property.ty),
                            has_implementation_body: false,
                            return_inference: None,
                        });
                    }
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    for parameter in &method.function.parameters {
                        if parameter.has_ts_annotation || parameter.is_parameter_property {
                            continue;
                        }
                        if let (Some(name), Some(span)) = (
                            parameter.name.as_deref(),
                            parameter.span.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }),
                        ) {
                            candidates.push(Candidate {
                                start: span.start,
                                name,
                                is_static: true,
                                position: TscInferredClassTypePosition::Parameter,
                                ty: Some(&parameter.ty),
                                has_implementation_body: method.has_implementation_body,
                                return_inference: None,
                            });
                        }
                    }
                    if method.function.spans.return_type.is_none()
                        && method.method_kind != verter_type_expr::ObjectMethodKind::Set
                    {
                        if let Some(span) = method.spans.declaration.filter(|span| {
                            span.start >= declaration_span.start && span.end <= declaration_span.end
                        }) {
                            candidates.push(Candidate {
                                start: span.start,
                                name: &method.name,
                                is_static: true,
                                position: TscInferredClassTypePosition::Return,
                                ty: method.function.return_type.as_deref(),
                                has_implementation_body: method.has_implementation_body,
                                return_inference: object_fact.members.get(member_index).and_then(
                                    |member| match member {
                                        verter_type_expr::facts::ObjectMemberFact::Method(
                                            method,
                                        ) => Some(method.function.return_inference),
                                        _ => None,
                                    },
                                ),
                            });
                        }
                    }
                }
                verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    for parameter in &function.parameters {
                        if parameter.has_ts_annotation || parameter.is_parameter_property {
                            continue;
                        }
                        if let (Some(name), Some(span)) = (
                            parameter.name.as_deref(),
                            parameter.span.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }),
                        ) {
                            candidates.push(Candidate {
                                start: span.start,
                                name,
                                is_static: false,
                                position: TscInferredClassTypePosition::Parameter,
                                ty: Some(&parameter.ty),
                                has_implementation_body: false,
                                return_inference: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    candidates.retain(|candidate| {
        !(candidate.has_implementation_body
            && overload_groups.contains(&(candidate.name.to_owned(), candidate.is_static)))
    });
    candidates.sort_by_key(|candidate| candidate.start);

    let mut occurrences = std::collections::BTreeMap::new();
    let mut inferred = Vec::with_capacity(candidates.len());
    let mut value_dependencies = BTreeSet::new();
    for candidate in candidates {
        if candidate.position == TscInferredClassTypePosition::Return {
            let Some(completeness) = candidate.return_inference else {
                return Err(ClassInferenceFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            };
            match completeness {
                verter_type_expr::facts::ReturnInferenceCompleteness::Unavailable(reason) => {
                    return Err(ClassInferenceFailure::InferenceUnavailable(reason));
                }
                verter_type_expr::facts::ReturnInferenceCompleteness::Unsupported(_) => {
                    return Err(ClassInferenceFailure::Unsupported(
                        UnsupportedReason::SemanticConstruct,
                    ));
                }
                verter_type_expr::facts::ReturnInferenceCompleteness::NotInferred
                | verter_type_expr::facts::ReturnInferenceCompleteness::Complete { .. } => {}
            }
        }
        let Some(candidate_type) = candidate.ty else {
            return Err(ClassInferenceFailure::Unsupported(
                UnsupportedReason::SemanticConstruct,
            ));
        };
        match crate::resolver_core::shallow_file_state::type_expr_is_declaration_safe(
            candidate_type,
        ) {
            Ok(true) => {}
            Ok(false) => {
                return Err(ClassInferenceFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            }
            Err(reason) => return Err(ClassInferenceFailure::InferenceUnavailable(reason)),
        }
        let occurrence = occurrences
            .entry((candidate.name, candidate.is_static, candidate.position))
            .or_insert(0_u32);
        let type_text = verter_type_expr::render_type_expr_display(candidate_type)
            .map_err(|_| ClassInferenceFailure::Unsupported(UnsupportedReason::SemanticConstruct))?
            .text;
        crate::resolver_core::shallow_file_state::collect_typeof_roots(
            candidate_type,
            &mut value_dependencies,
        )
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
        inferred.push(TscInferredClassMember {
            name: candidate.name.to_owned(),
            occurrence: *occurrence,
            is_static: candidate.is_static,
            position: candidate.position,
            type_text: TscSpliceText::new(type_text),
        });
        *occurrence = occurrence.saturating_add(1);
    }
    Ok(InferredClassProjection {
        members: inferred,
        value_dependencies,
    })
}

fn collect_local_declaration_closure(
    owner: TscScriptOwner,
    name: &str,
    inventory: &TscScopeInventory<'_>,
    required_imports: &mut BTreeMap<(TscScriptOwner, String), TscBindingUsage>,
    visiting: &mut BTreeSet<(TscScriptOwner, String)>,
    visited: &mut BTreeSet<(TscScriptOwner, String)>,
    owner_value_dependencies: &mut BTreeMap<
        (TscScriptOwner, String),
        BTreeSet<TscOwnerValueDependency>,
    >,
    retained_value_dependencies: &mut BTreeMap<
        (TscScriptOwner, String),
        BTreeSet<TscRetainedValueCarrier>,
    >,
    ordered: &mut Vec<(TscScriptOwner, String)>,
) -> Result<(), ProjectionFailure> {
    let identity = (owner, name.to_owned());
    if visited.contains(&identity) || !visiting.insert(identity.clone()) {
        return Ok(());
    }

    let declaration_owner = top_level_owner(owner);
    let Some(deps) = inventory
        .shallow_state
        .type_deps_in(declaration_owner, name)
    else {
        return Err(resolution_failure());
    };
    if !deps.unroutable_declaration_dependencies.is_empty() || deps.has_unroutable_value_position {
        return Err(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        ));
    }
    for external in &deps.declaration_external_deps {
        let usage = if deps.external_value_positions.contains(&external.local_name) {
            TscBindingUsage::ValuePosition
        } else if deps.external_value_queries.contains(&external.local_name) {
            TscBindingUsage::ValueQuery
        } else {
            TscBindingUsage::TypePosition
        };
        let import_owner = visible_import_owner(
            inventory,
            owner,
            &external.local_name,
            Some(&external.source_specifier),
        )?
        .ok_or(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ))?;
        merge_required_import(
            required_imports,
            (import_owner, external.local_name.clone()),
            usage,
        );
    }
    for dependency in &deps.owner_value_deps {
        let value_owner = local_value_owner(inventory, owner, dependency)?;
        owner_value_dependencies
            .entry(identity.clone())
            .or_default()
            .insert(TscOwnerValueDependency {
                owner: value_owner,
                name: dependency.clone(),
            });
    }
    for dependency in &deps.retained_value_carrier_deps {
        let value_owner = local_value_owner(inventory, owner, dependency)?;
        retained_value_dependencies
            .entry(identity.clone())
            .or_default()
            .insert(retained_value_carrier(inventory, value_owner, dependency)?);
    }

    let mut local_deps = deps.declaration_local_deps.clone();
    let mut local_deps = local_deps
        .drain(..)
        .map(|dependency| {
            let dependency_owner = visible_local_type_owner(inventory, owner, &dependency)?;
            Ok((dependency_owner, dependency))
        })
        .collect::<Result<Vec<_>, ProjectionFailure>>()?;
    local_deps.sort_by_key(|(owner, dependency)| declaration_order(inventory, *owner, dependency));
    local_deps.dedup();
    for (dependency_owner, dependency) in local_deps {
        if !inventory
            .shallow_state
            .effective_type_header_present_in(top_level_owner(dependency_owner), &dependency)
        {
            return Err(ProjectionFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
        collect_local_declaration_closure(
            dependency_owner,
            &dependency,
            inventory,
            required_imports,
            visiting,
            visited,
            owner_value_dependencies,
            retained_value_dependencies,
            ordered,
        )?;
    }

    visiting.remove(&identity);
    if visited.insert(identity.clone()) {
        ordered.push(identity);
    }
    Ok(())
}

fn retained_value_carrier(
    inventory: &TscScopeInventory<'_>,
    owner: TscScriptOwner,
    name: &str,
) -> Result<TscRetainedValueCarrier, ProjectionFailure> {
    inventory
        .analysis
        .declaration_entries
        .iter()
        .filter(|entry| {
            entry.owner == top_level_owner(owner)
                && entry.name == name
                && matches!(
                    entry.kind,
                    LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                )
        })
        .enumerate()
        .filter(|(_, entry)| entry.kind == LocalDeclarationKind::TypeAndValue)
        .last()
        .and_then(|(contributor_ordinal, _)| {
            u32::try_from(contributor_ordinal)
                .ok()
                .map(|contributor_ordinal| TscRetainedValueCarrier {
                    owner,
                    name: name.to_owned(),
                    contributor_ordinal,
                })
        })
        .ok_or(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ))
}

fn merge_required_import(
    required_imports: &mut BTreeMap<(TscScriptOwner, String), TscBindingUsage>,
    key: (TscScriptOwner, String),
    usage: TscBindingUsage,
) {
    required_imports
        .entry(key)
        .and_modify(|existing| {
            if binding_usage_precedence(usage) > binding_usage_precedence(*existing) {
                *existing = usage;
            }
        })
        .or_insert(usage);
}

fn binding_usage_precedence(usage: TscBindingUsage) -> u8 {
    match usage {
        TscBindingUsage::TypePosition => 0,
        TscBindingUsage::ValueQuery => 1,
        TscBindingUsage::ValuePosition => 2,
    }
}

#[allow(clippy::too_many_arguments)]
fn tsc_emit_rows(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &TypeInfoSurface,
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    counters: &mut VueMacroCodegenCounters,
) -> Result<Vec<TscEmitRow>, ProjectionFailure> {
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let mut rows = Vec::new();

    for signature in surface.call_signatures.iter() {
        let callable = CallableNodeView::new(dispatch, signature.node);
        let Some(names) = callable.event_names(context) else {
            continue;
        };
        let Some(signature) = callable.signature(context) else {
            continue;
        };
        let parameters = render_function_parameters(ctx, &signature.raw_params()[1..], counters)?;
        for name in names {
            push_tsc_emit(
                &mut rows,
                name.as_ref(),
                parameters.clone(),
                authored_emit_anchor(mac, payload_index, effective_index, name.as_ref()),
            );
        }
    }

    for member in surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
    {
        let parameters = render_emit_payload_parameters(ctx, dispatch, member.value, counters)?;
        push_tsc_emit(
            &mut rows,
            member.name.as_ref(),
            parameters,
            authored_emit_anchor(mac, payload_index, effective_index, member.name.as_ref()),
        );
    }

    for field in &mac.emit_fields {
        push_tsc_emit(
            &mut rows,
            field.name.as_str(),
            TscSpliceText::new("...args: unknown[]"),
            authored_emit_anchor(mac, payload_index, effective_index, field.name.as_str()),
        );
    }
    rows.sort_by_key(|row| authored_emit_order(row.anchor));

    if crate::request_context::current_cold_compute_completeness().is_partial() {
        return Err(partial_failure());
    }
    Ok(rows)
}

fn push_tsc_emit(
    rows: &mut Vec<TscEmitRow>,
    name: &str,
    parameters: TscSpliceText,
    anchor: MacroAnchor,
) {
    if rows.iter().any(|row| row.name == name) {
        return;
    }
    rows.push(TscEmitRow {
        name: name.to_owned(),
        emit_parameters: parameters.clone(),
        handler_parameters: parameters,
        anchor,
    });
}

fn render_emit_payload_parameters(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    use crate::semantic_query::SemanticNodeData;

    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let Some(node) = dispatch
        .normalize_node_for_structural_fact_demand(node, context)
        .into_complete_node()
    else {
        return Err(
            if crate::request_context::current_cold_compute_completeness().is_partial() {
                partial_failure()
            } else {
                resolution_failure()
            },
        );
    };
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
        Some(SemanticNodeData::Tuple { elements, .. }) => {
            render_tuple_parameters(ctx, elements, counters)
        }
        Some(SemanticNodeData::Function { params, .. }) => {
            render_function_parameters(ctx, params, counters)
        }
        _ => Ok(TscSpliceText::new("...args: unknown[]")),
    }
}

fn render_tuple_parameters(
    ctx: &dyn ResolverContext,
    elements: &[crate::semantic_query::TupleElement],
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    let mut rendered = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let ty = render_tsc_node(ctx, element.value, counters)?;
        let name = element
            .label
            .as_deref()
            .map_or_else(|| format!("arg{index}"), ToOwned::to_owned);
        rendered.push(render_tsc_parameter(
            &name,
            ty.as_str(),
            element.optional,
            element.rest,
        ));
    }
    Ok(TscSpliceText::new(rendered.join(", ")))
}

fn render_function_parameters(
    ctx: &dyn ResolverContext,
    params: &[crate::semantic_query::FunctionParam],
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    let mut rendered = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let ty = render_tsc_node(ctx, param.ty, counters)?;
        let name = param
            .name
            .as_deref()
            .map_or_else(|| format!("arg{index}"), ToOwned::to_owned);
        rendered.push(render_tsc_parameter(
            &name,
            ty.as_str(),
            param.optional,
            param.rest,
        ));
    }
    Ok(TscSpliceText::new(rendered.join(", ")))
}

fn render_tsc_parameter(name: &str, ty: &str, optional: bool, rest: bool) -> String {
    format!(
        "{}{}{}: {}",
        if rest { "..." } else { "" },
        name,
        if optional && !rest { "?" } else { "" },
        ty
    )
}

/// Prove a row-local missing dependency without conflating it with an
/// authored `unknown` or with a reference nested below a constructor-bearing
/// shell. The semantic analyzer is the authority for which imported heads are
/// MEMBER-tier dependencies; this walk only follows the transparent shapes
/// that participate in broad constructor inference.
fn direct_member_dependency_is_missing(
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
fn probe_definitely_non_object_root(
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

struct RuntimeClassification {
    constructors: OrderedRuntimeConstructors,
    skip_check: bool,
}

fn classify_runtime(
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

fn emit_rows(
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

fn authored_emit_order(anchor: MacroAnchor) -> (u8, u32) {
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

fn member_anchor(mac: &AnalyzedMacro, payload_index: usize, name: &str) -> MacroAnchor {
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

fn authored_emit_anchor(
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

fn defaults_association(
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

fn top_level_syntax_index(macros: &[AnalyzedMacro], effective_index: usize) -> u32 {
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
