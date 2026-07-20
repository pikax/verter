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

mod runtime;
mod tsc_projection;

use runtime::*;
use tsc_projection::*;

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

/// Typed missing-dependency provenance produced by the same graph projection
/// that builds the macro DTOs. Host compile converts this list to diagnostics;
/// neither the compiler nor the host infers dependency failures from generic
/// partiality or display text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum VueMacroDependencyFailure {
    /// The authored macro root is an imported surface-tier dependency whose
    /// semantic payload could not be established.
    MissingRoot {
        /// Analyzer macro inventory index.
        macro_index: usize,
        /// Exact top-level lexical owner of the macro/import binding.
        owner: verter_type_expr::TopLevelOwnerId,
        /// Exact authored import source.
        import_source: String,
        /// Exact authored local type name.
        type_name: String,
    },
    /// A resolved root dropped one imported heritage/intersection/union arm.
    UnresolvedSurfaceArm {
        /// Analyzer macro inventory index.
        macro_index: usize,
        /// Exact top-level lexical owner of the consuming macro.
        macro_owner: verter_type_expr::TopLevelOwnerId,
        /// Unresolved arm head.
        name: Arc<str>,
        /// Canonical file that authored the arm.
        owner_canonical: Arc<str>,
        /// Exact top-level lexical owner that authored the arm/import.
        owner: verter_type_expr::TopLevelOwnerId,
    },
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
    /// Sorted, deduplicated typed missing-dependency failures. This is
    /// demand-invariant: Runtime, TSC, and combined requests report the same
    /// semantic failure inventory.
    pub dependency_failures: Vec<VueMacroDependencyFailure>,
    /// Typed structural completeness observed during this invocation.
    /// Deterministic-instrumentation contract surface: the per-ENTRY typed
    /// outcomes inside the bundles are the compiler-facing semantics; this
    /// aggregate is read by the in-crate contract/perf suites, not by the
    /// compile pipeline.
    #[allow(dead_code)]
    pub completeness: ResultCompleteness,
    /// Whether the fact footprint was bounded and based only on publishable
    /// reads. Same deterministic-instrumentation contract surface as
    /// `completeness`.
    #[allow(dead_code)]
    pub facts_cacheable: bool,
    /// Deterministic work counters (the flake-free performance-contract
    /// rail: one producer invocation, one root shallow demand, no per-prop
    /// scheduler fan-out — asserted by the contract suites).
    #[allow(dead_code)]
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
    dependency_failures: Vec<VueMacroDependencyFailure>,
    counters: VueMacroCodegenCounters,
    completeness: ResultCompleteness,
}

fn record_unresolved_surface_arms(
    failures: &mut Vec<VueMacroDependencyFailure>,
    macro_index: usize,
    macro_owner: verter_type_expr::TopLevelOwnerId,
    diagnostics: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
) {
    failures.extend(diagnostics.iter().filter_map(|diagnostic| {
        let crate::project_semantic_dispatch::walk::ShallowDiagnostic::UnresolvedSurfaceArm {
            name,
            owner_canonical,
            owner,
        } = diagnostic
        else {
            return None;
        };
        Some(VueMacroDependencyFailure::UnresolvedSurfaceArm {
            macro_index,
            macro_owner,
            name: Arc::clone(name),
            owner_canonical: Arc::clone(owner_canonical),
            owner: *owner,
        })
    }));
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
        dependency_failures: Vec::new(),
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
    /// Produce the compiler-facing [`VueMacroSemanticInput`] for one carrier and
    /// compile target.
    ///
    /// This is the SAME authoritative-bundle handoff the host's own audited
    /// compile path uses (`vue_macro_compile_input`), exposed publicly so an
    /// out-of-crate consumer that compiles a carrier directly — the `verter-tsc`
    /// validation-carrier stage — threads the real semantic bundle instead of
    /// [`VueMacroSemanticInput::Unavailable`]. Without it, a type-based macro's
    /// template prop references degrade to instance-property access
    /// (`___VERTER___instance.foo`) instead of the resolved `__props.foo` form.
    #[must_use]
    pub fn vue_macro_semantic_input(
        &self,
        canonical_id: &str,
        target: verter_compiler::compile::CompileTarget,
    ) -> verter_compiler::compile::VueMacroSemanticInput {
        let demand = match (target.needs_runtime_macro_semantics(), target.needs_tsc()) {
            (true, true) => Some(VueMacroCodegenDemand::RuntimeAndTsc),
            (true, false) => Some(VueMacroCodegenDemand::Runtime),
            (false, true) => Some(VueMacroCodegenDemand::Tsc),
            (false, false) => None,
        };
        demand
            .map(|demand| {
                self.produce_vue_macro_codegen(canonical_id, demand)
                    .compiler_input()
            })
            .unwrap_or(verter_compiler::compile::VueMacroSemanticInput::Unavailable)
    }

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
            dependency_failures: state.dependency_failures,
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
            dependency_failures: Vec::new(),
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
            if payload.is_none() {
                state.dependency_failures.extend(
                    script_analysis
                        .macro_type_deps
                        .iter()
                        .filter(|dependency| {
                            dependency.macro_index == payload_index
                                && dependency.macro_span == mac.span
                                && dependency.usage.is_surface()
                        })
                        .map(|dependency| VueMacroDependencyFailure::MissingRoot {
                            macro_index: payload_index,
                            owner: mac.owner,
                            import_source: dependency.import_source.clone(),
                            type_name: dependency.type_name.clone(),
                        }),
                );
            }
            let payload_failure =
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    Some(partial_failure())
                } else if payload.is_none() {
                    Some(resolution_failure())
                } else {
                    None
                };

            if demand.wants_runtime() {
                let mut walker_diagnostics = Vec::new();
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
                                        &mut walker_diagnostics,
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
                                &mut walker_diagnostics,
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
                record_unresolved_surface_arms(
                    &mut state.dependency_failures,
                    payload_index,
                    mac.owner,
                    &walker_diagnostics,
                );
                state.runtime_entries.push(MacroRuntimeEntry {
                    syntax_index,
                    macro_index,
                    outcome,
                });
            }

            if demand.wants_tsc() {
                let mut walker_diagnostics = Vec::new();
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
                            owner_canonical,
                            &tsc_scope_inventory,
                            &mut state.counters,
                            &mut walker_diagnostics,
                        ),
                        (None, None) => unreachable!("payload failure covers an absent payload"),
                    }
                };
                record_unresolved_surface_arms(
                    &mut state.dependency_failures,
                    payload_index,
                    mac.owner,
                    &walker_diagnostics,
                );
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

        state.dependency_failures.sort();
        state.dependency_failures.dedup();
        state
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn project_tsc_macro(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        payload: crate::semantic_query::SemanticNodeId,
        mac: &AnalyzedMacro,
        payload_index: usize,
        effective_index: usize,
        owner_canonical: &str,
        scope_inventory: &TscScopeInventory<'_>,
        counters: &mut VueMacroCodegenCounters,
        walker_diagnostics: &mut Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic>,
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
                    Some(&mut *walker_diagnostics),
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
                    let type_text = if let Some(import_form) = cross_file_namespace_import_type(
                        ctx,
                        dispatch,
                        member.value,
                        owner_canonical,
                        scope_inventory,
                    ) {
                        TscSpliceText::new(import_form)
                    } else {
                        match render_tsc_node(ctx, member.value, counters) {
                            Ok(text) => text,
                            Err(failure) => return failure.tsc(),
                        }
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
                    Some(&mut *walker_diagnostics),
                );
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return partial_failure().tsc();
                }
                let Some(surface) = surface else {
                    return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).tsc();
                };
                if emits_surface_has_invalid_member(
                    dispatch,
                    &surface,
                    ProjectionReductionContext::published(ProjectionMode::Navigate),
                ) {
                    return ProjectionFailure::Invalid(MacroInvalidReason::InvalidEmitsShape).tsc();
                }
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
                let mut scope = match tsc_scope_requirements(mac, scope_inventory) {
                    Ok(scope) => scope,
                    Err(failure) => return failure.tsc(),
                };
                // A `defineEmits<E>()` whose type argument is a bare reference to
                // a type declared in ANOTHER framework-carrier SFC
                // (`import type { E } from './Child.vue'; defineEmits<E>()`) is
                // re-synthesized into per-event rows — the imported `E` name is
                // never referenced in the generated output. Retaining
                // `import type { E } from './Child.vue'` would leave a DANGLING
                // type import: a `.vue` module resolves to `DefineComponent` via
                // the `*.vue` shim, never a named type export. Drop the now-unused
                // carrier emit-type binding (nested payload-type imports the rows
                // DO reference stay retained).
                if emit_type_is_cross_sfc_carrier(dispatch, payload, owner_canonical) {
                    scope.retained_bindings.retain(|binding| {
                        !mac.type_references
                            .iter()
                            .any(|name| name.as_str() == binding.local_name.as_str())
                    });
                }
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
        walker_diagnostics: &mut Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic>,
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
            Some(walker_diagnostics),
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
        walker_diagnostics: &mut Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic>,
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
            Some(walker_diagnostics),
        );
        if crate::request_context::current_cold_compute_completeness().is_partial() {
            return partial_failure().runtime();
        }
        let Some(surface) = surface else {
            return ProjectionFailure::Invalid(MacroInvalidReason::NonObjectRoot).runtime();
        };
        if emits_surface_has_invalid_member(dispatch, &surface, runtime_context) {
            return ProjectionFailure::Invalid(MacroInvalidReason::InvalidEmitsShape).runtime();
        }

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
