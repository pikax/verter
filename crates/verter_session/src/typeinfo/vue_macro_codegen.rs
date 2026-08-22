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
    TscEmitsProjection, TscExposeMemberRow, TscExposeMemberType, TscExposeProjection,
    TscInferredClassMember, TscInferredClassTypePosition, TscModelProjection,
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
use crate::resolver_core::{
    FactReadSetFinalise, FactVersionRef, ResolverContext, StoreViewCompatToken,
};
use crate::semantic_query::{
    BroadRuntimeKind, PartialReasonSet, PathSegment, ProjectionMode, ProjectionReductionContext,
    QueryResult, ResolveDeclKey, ResultCompleteness, ScopeId, SemanticNodeData, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryValue, SurfaceProvenanceContext, ValueRootKey,
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
    /// Runtime names and optionality WITHOUT broad constructor
    /// classification.
    ///
    /// A TSX-only (IDE) compile reads the public binding names out of this
    /// bundle and never reads `RuntimeProp::type_shape`. Classifying a
    /// member's broad constructor resolves that member's entire type through
    /// the shared semantic engine, which is why the interactive LSP path does
    /// not ask for it.
    RuntimeBindingNames,
    /// Terminal TSC/IDE splice text only.
    Tsc,
    /// Both independent bundles in one inventory pass.
    RuntimeAndTsc,
}

impl VueMacroCodegenDemand {
    /// The single target -> demand mapping every compile entry point uses.
    ///
    /// `None` means the target consumes no macro semantics at all.
    pub(crate) fn for_compile_target(target: crate::CompileTarget) -> Option<Self> {
        match (
            target.needs_runtime_macro_semantics(),
            target.needs_runtime_prop_constructors(),
            target.needs_tsc(),
        ) {
            (true, _, true) => Some(Self::RuntimeAndTsc),
            (true, true, false) => Some(Self::Runtime),
            (true, false, false) => Some(Self::RuntimeBindingNames),
            (false, _, true) => Some(Self::Tsc),
            (false, _, false) => None,
        }
    }

    const fn wants_runtime(self) -> bool {
        matches!(
            self,
            Self::Runtime | Self::RuntimeBindingNames | Self::RuntimeAndTsc
        )
    }

    /// Whether the runtime bundle must carry per-member broad constructors.
    ///
    /// This is the single gate that keeps `ClassifyBroadRuntime` — and the
    /// whole member-type resolution it forces — off a TSX-only compile.
    const fn wants_runtime_constructors(self) -> bool {
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
            Self::RuntimeBindingNames => 3,
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
        /// The originating macro call's own span (`(start, end)`) — the
        /// real fallback anchor when the precise import binding cannot be
        /// relocated. A plain tuple, not `verter_span::Span`, because this
        /// enum derives `Ord` and `Span` does not.
        macro_span: (u32, u32),
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
    /// The producer's finalised fact observation set, carried back for the
    /// consuming thread to replay. See [`MacroFactFootprint`].
    pub fact_footprint: MacroFactFootprint,
    /// Deterministic work counters (the flake-free performance-contract
    /// rail: one producer invocation, one root shallow demand, no per-prop
    /// scheduler fan-out — asserted by the contract suites).
    #[allow(dead_code)]
    pub counters: VueMacroCodegenCounters,
}

/// The producer's finalised fact observation set, carried across the scheduler
/// worker boundary.
///
/// [`VerterHost::produce_vue_macro_codegen_with_ctx`] dispatches its build
/// closure through [`verter_scheduler::scheduler::Scheduler::execute_scoped_cache_node`],
/// which runs it on a scheduler CPU-pool worker. The fact tracer stack is
/// thread-local with no cross-thread bridge, so the tracer the producer installs
/// on that worker cannot reach the enclosing compute's tracer on the submitting
/// thread — and a caller that joins an in-flight rendezvous runs no closure at
/// all. This carrier holds the producer's observations plus its non-cacheable
/// refusal so [`Self::replay`] can restate both on the CONSUMING thread; it is
/// what roots the enclosing compile on every file the macro traversal actually
/// read, including the transitively reached ones no direct import names.
#[derive(Debug, Clone)]
pub(crate) enum MacroFactFootprint {
    /// A bounded observation set built only from publishable reads.
    Rooted(Arc<[FactVersionRef]>),
    /// A bounded observation set whose compute also consumed a read these facts
    /// cannot validate. The facts still bubble into enclosing scopes; they must
    /// never authorize shared-cache admission.
    RootedNonCacheable(Arc<[FactVersionRef]>),
    /// Finalisation exceeded the per-signature cap, so NO facts survived.
    /// Replaying that as an empty observation set would root the consumer on
    /// nothing — silently, and on exactly the wide-footprint inputs most likely
    /// to reach it — so this arm can only refuse.
    Overflowed,
    /// The producer's aggregate basis moved while its tracer was open.
    /// No fact set can validate that mixed world, so the consumer must
    /// refuse even though the producer did run.
    MutationUnstable,
    /// No traced producer compute ran (cancelled, shut down, faulted, or
    /// re-entrant), so nothing was observed and nothing may authorize rooting.
    Unobserved,
}

impl MacroFactFootprint {
    /// Split a producer tracer's finalised observation set into the transitive
    /// canonical inventory the compile pipeline syncs as macro type dependencies
    /// and the replayable footprint carrier.
    pub(crate) fn from_finalise(finalise: FactReadSetFinalise) -> (Vec<String>, Self) {
        let footprint = match finalise {
            FactReadSetFinalise::Ok(facts) => Self::Rooted(facts),
            FactReadSetFinalise::NonCacheable(facts) => Self::RootedNonCacheable(facts),
            FactReadSetFinalise::Overflow => Self::Overflowed,
            FactReadSetFinalise::MutationUnstable => Self::MutationUnstable,
        };
        let canonicals: BTreeSet<String> = footprint
            .facts()
            .iter()
            .filter_map(FactVersionRef::canonical_id)
            .map(ToOwned::to_owned)
            .collect();
        (canonicals.into_iter().collect(), footprint)
    }

    /// The surviving observations. Every factless arm yields an empty slice —
    /// they carry a refusal, never a signature.
    fn facts(&self) -> &[FactVersionRef] {
        match self {
            Self::Rooted(facts) | Self::RootedNonCacheable(facts) => facts,
            Self::Overflowed | Self::MutationUnstable | Self::Unobserved => &[],
        }
    }

    /// Restate the producer's footprint onto the CURRENT thread's tracer stack.
    ///
    /// Idempotent: finalisation canonicalises before enforcing the cap, so a
    /// re-observed fact dedups away rather than inflating the consumer's set.
    /// Every arm is handled explicitly — a factless arm must taint, because a
    /// consumer that roots on nothing serves stale results forever.
    pub(crate) fn replay(&self) {
        use crate::resolver_core::fact_read_set::NonCacheablePropagation;
        use crate::resolver_core::resolver_context;
        match self {
            Self::Rooted(facts) => resolver_context::observe_fan_out_borrowed(facts),
            Self::RootedNonCacheable(facts) => {
                resolver_context::observe_fan_out_borrowed(facts);
                resolver_context::note_non_cacheable_propagation(
                    NonCacheablePropagation::Transitive,
                );
            }
            Self::Overflowed | Self::MutationUnstable | Self::Unobserved => {
                resolver_context::note_non_cacheable_propagation(
                    NonCacheablePropagation::Transitive,
                );
            }
        }
    }
}

impl VueMacroCodegenOutput {
    /// Whether the producer's footprint was bounded and built only from
    /// publishable reads — the deterministic-instrumentation contract surface
    /// the in-crate suites assert, derived from the carrier that drives replay.
    #[cfg(test)]
    pub(crate) fn facts_cacheable(&self) -> bool {
        matches!(self.fact_footprint, MacroFactFootprint::Rooted(_))
    }

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

    /// Per-member `defineExpose` runtime-object degradation reason. There is
    /// no per-member `Partial`/`Invalid` row in [`TscDeclarationFailureReason`]
    /// — a mid-resolution partial (budget exhaustion, cancellation) reports
    /// as an honest `Unresolved` degradation rather than inventing a new
    /// taxonomy row for one caller, and `Invalid` (a root-shape rejection
    /// that has no meaning for a single value binding) folds to the same
    /// `Unsupported(SemanticConstruct)` catch-all [`Self::member`] already
    /// uses.
    fn expose_declaration_reason(self) -> TscDeclarationFailureReason {
        match self {
            Self::Partial(_) => {
                TscDeclarationFailureReason::Unresolved(UnresolvedReason::MissingDependency)
            }
            Self::Unresolved(reason) => TscDeclarationFailureReason::Unresolved(reason),
            Self::Unsupported(reason) => TscDeclarationFailureReason::Unsupported(reason),
            Self::Invalid(_) => {
                TscDeclarationFailureReason::Unsupported(UnsupportedReason::SemanticConstruct)
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
            // Runtime-object `defineExpose` never has a runtime shape (see
            // `is_codegen_macro`), so it advertises a TSC-only Partial row
            // here, matching the compute path's own syntax_index so
            // `apply_tsc_bundle`'s bundle-vs-advertised-entries accounting
            // stays consistent across the cancelled/terminal-partial lane.
            if mac.kind == AnalyzedMacroKind::DefineExpose
                && !mac.is_type_based
                && !mac.expose_fields.is_empty()
            {
                if demand.wants_tsc() {
                    tsc_entries.push(MacroTscEntry {
                        syntax_index: top_level_syntax_index(macros, payload_index),
                        macro_index: macro_index(payload_index),
                        outcome: MacroTscOutcome::Partial(MacroFailure::new(macro_reason, None)),
                    });
                }
                continue;
            }
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
        // No traced compute ran, so this handoff roots nothing: replaying it
        // refuses the consumer's admission rather than pretending an empty
        // signature validated.
        fact_footprint: MacroFactFootprint::Unobserved,
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
        target: crate::CompileTarget,
    ) -> verter_compiler::compile::VueMacroSemanticInput {
        VueMacroCodegenDemand::for_compile_target(target)
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

        let output = match self
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
        };
        // The ONLY point at which the producer's observations reach this
        // thread. The build ran on a scheduler CPU-pool worker (or, for a
        // caller that joined the in-flight rendezvous, did not run here at
        // all), and the tracer stack is thread-local — so without this the
        // enclosing compute roots on nothing the macro traversal read past its
        // direct imports, and any refusal the producer raised is lost.
        output.fact_footprint.replay();
        output
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
        let (mut state, finalise) = crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            || {
                let _completeness_scope =
                    crate::request_context::ColdComputeCompletenessScope::enter();
                let mut state = self.produce_vue_macro_codegen_inner(ctx, owner_canonical, demand);
                // The producer's OWN completeness, with the CONTAINED
                // classes subtracted: a body-derived return this substrate
                // could not infer does not make the emitted TSX an
                // incomplete surface (the declarations ride verbatim and
                // the external checker types them), so it must not report
                // the file-level result partial either. Every other reason
                // class survives — including the declaration-local budget
                // classification the class-member inference records
                // precisely on its own rail.
                //
                // The FILE-level aggregate contains everything the two
                // lanes contain BETWEEN them: a lane that refused recorded
                // the refusal IN its entry, so the artifact is a faithful,
                // deterministic record of it and warming that record is
                // correct. The per-lane sets are what the two projections
                // read, and they differ — see `MacroProjectionLane`.
                let observed = crate::request_context::current_cold_compute_completeness();
                let residual = macro_projection_residual(observed, MacroProjectionLane::File);
                state.completeness = if residual.is_empty() {
                    crate::semantic_query::ResultCompleteness::Complete
                } else {
                    crate::semantic_query::ResultCompleteness::Partial(residual)
                };
                state
            },
        );
        state.counters.scheduler_submissions = 1;

        let (transitive_canonicals, fact_footprint) = MacroFactFootprint::from_finalise(finalise);
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
            fact_footprint,
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
            // Runtime-object `defineExpose({ ... })`: never a runtime shape
            // (see `is_codegen_macro`'s doc comment), no macro type argument
            // to resolve a payload from, so this is a dedicated TSC-only lane
            // rather than a fourth arm bolted onto the payload/runtime flow
            // Props/Emits/Model share. The type-argument form
            // (`defineExpose<T>()`, `mac.is_type_based`) is unchanged: the
            // compiler still splices it verbatim from authored syntax.
            if mac.kind == AnalyzedMacroKind::DefineExpose
                && !mac.is_type_based
                && !mac.expose_fields.is_empty()
            {
                if demand.wants_tsc() {
                    let syntax_index = top_level_syntax_index(macros, payload_index);
                    let macro_index = macro_index(payload_index);
                    let macro_scope = crate::request_context::ColdComputeCompletenessScope::enter();
                    let outcome = self.project_expose_runtime_object(
                        ctx,
                        &dispatch,
                        mac,
                        payload_index,
                        owner_canonical,
                        &tsc_scope_inventory,
                        &mut state.counters,
                    );
                    let macro_completeness =
                        crate::request_context::current_cold_compute_completeness();
                    macro_scope.discard();
                    crate::request_context::fold_result_completeness(macro_completeness);
                    state.tsc_entries.push(MacroTscEntry {
                        syntax_index,
                        macro_index,
                        outcome,
                    });
                }
                continue;
            }
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
                            macro_span: (dependency.macro_span.start, dependency.macro_span.end),
                        }),
                );
            }
            // PER-LANE payload failure, over the PER-LANE contained class
            // set: the runtime lane derives its output from the value, so
            // a producer that yielded no surface faults it while the TSC
            // lane's authored splice rides on. The two lanes also differ
            // on an ABSENT payload — the TSC lane splices the authored
            // declaration and reports a resolution failure, while the
            // runtime lane has nothing to derive a `props` option object
            // from. They are computed separately and never merged.
            let runtime_lane = MacroProjectionLane::runtime(demand.wants_runtime_constructors());
            let tsc_payload_failure = if macro_projection_faulted(MacroProjectionLane::Tsc) {
                Some(partial_failure())
            } else if payload.is_none() {
                Some(resolution_failure(MacroProjectionLane::Tsc))
            } else {
                None
            };
            let runtime_payload_failure = if macro_projection_faulted(runtime_lane) {
                Some(partial_failure())
            } else if payload.is_none() {
                Some(resolution_failure(runtime_lane))
            } else {
                None
            };

            if demand.wants_runtime() {
                let mut walker_diagnostics = Vec::new();
                let outcome = {
                    let _runtime_scope =
                        crate::request_context::ColdComputeCompletenessScope::enter();
                    match (runtime_payload_failure, payload) {
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
                                        demand.wants_runtime_constructors(),
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
                                demand.wants_runtime_constructors(),
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
                                        demand.wants_runtime_constructors(),
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
                    match (tsc_payload_failure, payload) {
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
                if macro_projection_faulted(MacroProjectionLane::Tsc) {
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
                    let Some(member_name) = member.published_name() else {
                        continue;
                    };
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
                        name: member_name.to_string(),
                        optional: member.optional,
                        type_text,
                        anchor: member_anchor(mac, payload_index, member_name.as_ref()),
                    });
                }
                let scope = match tsc_scope_requirements(
                    mac,
                    scope_inventory,
                    ctx,
                    dispatch,
                    owner_canonical,
                ) {
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
                if macro_projection_faulted(MacroProjectionLane::Tsc) {
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
                let mut scope = match tsc_scope_requirements(
                    mac,
                    scope_inventory,
                    ctx,
                    dispatch,
                    owner_canonical,
                ) {
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
                let scope = match tsc_scope_requirements(
                    mac,
                    scope_inventory,
                    ctx,
                    dispatch,
                    owner_canonical,
                ) {
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

    /// TSC projection for a runtime-object `defineExpose({ ... })` macro —
    /// the ONLY expose form this producer projects. The type-argument form
    /// (`defineExpose<T>()`) is spliced verbatim by the compiler from
    /// authored syntax and never reaches this producer (`DefineExpose` is
    /// deliberately absent from [`is_codegen_macro`]: it has no runtime
    /// `props`/`emits` shape, so it never enters the shared
    /// payload/runtime-projection flow those roles share).
    ///
    /// Each member with a structurally captured [`AnalyzedExposeField::referenced_binding`]
    /// resolves through the SAME shared `TypeOf` query every other
    /// value-typeof consumer uses (`ProjectSemanticDispatch::typeof_key_for`)
    /// — never a second resolver, never text-based inference. A member with
    /// no capturable binding (a method, a non-identifier value expression)
    /// or whose `TypeOf` resolution genuinely misses reports a typed
    /// [`TscExposeMemberType::Unavailable`] row instead of a silent
    /// `unknown` masquerading as success; the compiler falls back to its own
    /// authored-syntax-derived type (or `unknown`) for that member.
    fn project_expose_runtime_object(
        &self,
        ctx: &dyn ResolverContext,
        dispatch: &ProjectSemanticDispatch<'_>,
        mac: &AnalyzedMacro,
        payload_index: usize,
        owner_canonical: &str,
        scope_inventory: &TscScopeInventory<'_>,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroTscOutcome {
        let mut members = Vec::with_capacity(mac.expose_fields.len());
        let mut ref_names: FxHashSet<String> = FxHashSet::default();
        for (field_index, field) in mac.expose_fields.iter().enumerate() {
            let member_type = match &field.referenced_binding {
                Some(binding_name) => {
                    let key = dispatch.typeof_key_for(
                        ValueRootKey {
                            scope: ScopeId::file(Arc::from(owner_canonical), mac.owner),
                            name: Arc::from(binding_name.as_str()),
                        },
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    );
                    let read = dispatch.execute_read(key);
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(node) | QueryResult::Recursive(node) => {
                            let context =
                                ProjectionReductionContext::published(ProjectionMode::Expanded);
                            let reduced = dispatch.reduce_output_node_with_context(node, context);
                            if reduced.result_is_partial() {
                                crate::request_context::mark_request_result_partial();
                            }
                            // `render_tsc_node` itself fails closed (typed
                            // `Err`) on a NESTED resolver degradation baked
                            // into the rendered text — see its doc comment.
                            // This producer never sees a leaked sentinel
                            // masquerading as `Ok` text, so it needs no
                            // second, textual screen of its own; a genuinely
                            // resolved member is the only `Ok` outcome.
                            match render_tsc_node(ctx, reduced.node_id(), counters) {
                                Ok(text) => {
                                    crate::resolver_core::component_meta_registry::collect_node_ref_names(
                                        ctx,
                                        reduced.node_id(),
                                        &mut ref_names,
                                    );
                                    TscExposeMemberType::Resolved(text)
                                }
                                Err(failure) => TscExposeMemberType::Unavailable(
                                    failure.expose_declaration_reason(),
                                ),
                            }
                        }
                        QueryResult::Error(_) => TscExposeMemberType::Unavailable(
                            TscDeclarationFailureReason::Unresolved(
                                UnresolvedReason::MissingDeclaration,
                            ),
                        ),
                    }
                }
                None => TscExposeMemberType::Unavailable(TscDeclarationFailureReason::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                )),
            };
            members.push(TscExposeMemberRow {
                name: field.name.clone(),
                member_type,
                anchor: expose_member_anchor(mac, payload_index, field_index),
            });
        }
        let type_references: Vec<String> = ref_names.into_iter().collect();
        let scope = match tsc_scope_requirements_for(
            mac.owner,
            None,
            &type_references,
            scope_inventory,
            ctx,
            dispatch,
            owner_canonical,
        ) {
            Ok(scope) => scope,
            Err(failure) => return failure.tsc(),
        };
        MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
            members,
            scope,
        }))
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
        classify_constructors: bool,
        counters: &mut VueMacroCodegenCounters,
        walker_diagnostics: &mut Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic>,
    ) -> MacroRuntimeOutcome {
        let lane = MacroProjectionLane::runtime(classify_constructors);
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
        if macro_projection_faulted(lane) {
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
            let Some(member_name) = member.published_name() else {
                continue;
            };
            // A demand that never renders the runtime `props` option object
            // asks nothing about the member's type: the whole per-member
            // classification chain — and the cross-file type resolution it
            // forces — is skipped rather than computed and discarded.
            let type_shape = if !classify_constructors {
                RuntimePropType::Unclassified
            } else if direct_member_dependency_is_missing(
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
                    runtime_subject.member(Arc::clone(&member_name)),
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
                name: member_name.to_string(),
                optional: member.optional,
                type_shape,
                anchor: member_anchor(mac, payload_index, member_name.as_ref()),
            });
        }

        // A ROOT-position degradation leaves NO member set. The class is
        // the same one an interior position records — only the derived
        // surface tells them apart, which is why this is a structural check
        // on the surface and not a reason-class subtraction.
        //
        // An empty surface is a legitimate answer for `defineProps<{}>()`
        // and only for it; under an observed flow-return degradation the
        // same emptiness means the substrate could not type the root, and
        // emitting `props: {}` then declares a props-less component for a
        // component that declares props — every listener and bound
        // attribute silently falls through to `$attrs`. Refusing is loud,
        // and the TSX lane still type-checks the file.
        //
        // Scoped to the demand that RENDERS the option object. A names-only
        // (TSX / IDE) compile reads the public binding names and renders no
        // `props` option at all, and its generated TSX splices the AUTHORED
        // macro call for the external checker — so refusing there would
        // delete the whole file's type-check surface to prevent an option
        // object that demand never emits.
        if classify_constructors && props.is_empty() && flow_return_degradation_observed() {
            return partial_failure().runtime();
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
        renders_runtime_options: bool,
        counters: &mut VueMacroCodegenCounters,
        walker_diagnostics: &mut Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic>,
    ) -> MacroRuntimeOutcome {
        let lane = MacroProjectionLane::runtime(renders_runtime_options);
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
        if macro_projection_faulted(lane) {
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
        if macro_projection_faulted(lane) {
            return partial_failure().runtime();
        }
        // The `props` twin of this refusal, for the same reason and with
        // the same scope: a ROOT-position degradation leaves no member set,
        // and `emits: []` for a component that declares emits sends every
        // listener silently through to `$attrs`. `renders_runtime_options`
        // keeps a names-only (TSX / IDE) compile out of it — that demand
        // emits no `emits` option and its TSX splices the authored macro
        // call for the external checker.
        if renders_runtime_options && emits.is_empty() && flow_return_degradation_observed() {
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
        classify_constructors: bool,
        counters: &mut VueMacroCodegenCounters,
    ) -> MacroRuntimeOutcome {
        let value_type_shape = if classify_constructors {
            match classify_runtime(dispatch, runtime_subject, counters) {
                Ok(classification) => RuntimePropType::Resolved {
                    constructors: classification.constructors,
                    skip_check: classification.skip_check,
                },
                Err(failure) => return failure.runtime(),
            }
        } else {
            RuntimePropType::Unclassified
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
                type_shape: value_type_shape,
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
