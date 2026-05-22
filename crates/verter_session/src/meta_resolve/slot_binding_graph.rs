//! Graph-native slot-binding synthesis.
//!
//! Replaces the parser-side enrichment step with a query-driven
//! synthesis that walks `defineSlots` macro payloads through
//! `ProjectSemanticDispatch`'s empty-path Shallow surface, then
//! enumerates each slot's binding parameters from the same shared
//! graph.
//!
//! # Design summary
//!
//! - Each `defineSlots` invocation is identified by
//!   [`SlotMacroIdentity`] = `(owner_canonical, macro_index, type_args)`.
//! - The macro payload's empty-path Shallow surface enumerates slot
//!   members. For every slot member, the shared graph walks
//!   `Function.params[0].ty`'s Shallow surface to enumerate bindings.
//! - Bindings carry a [`SlotBindingSource`] discriminator so
//!   [`publish_merged_bindings`] can decide whether the parser-path
//!   `raw_type` is the correct backstop.
//! - Synthesis returns a [`SynthesisResult`] so the caller can gate
//!   `ComponentMetaResultDb` publication when a fatal error suppressed
//!   the run.

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{
    ExpandedComponentTypes, ExpandedField, ExpansionDiagnostic, ExpansionExactness,
    ExpansionExecutionStatus, ExpansionStopReason,
};
use verter_semantic::analysis::AnalyzedMacroKind;
use verter_type_expr::TypeExpr;

use super::dep_signature::accumulate_dispatch_dep_signature;
use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::component_meta::ResolvedMacroMeta;
use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
use crate::resolver_core::ResolverContext;

/// Paired emission helper for the five dispatch-read fact observation
/// sites in this file.
///
/// The slot-binding-graph traversal has no result cache of its own;
/// the dispatch reads' facts reach two downstream channels:
///
/// 1. The legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` (TLS), drained
///    at `host_manage/component_meta_methods.rs::compute_component_meta_state_inner`
///    and folded into `state.fact_versions` →
///    `ComponentMetaResultEntry.fact_dep_signature` (via
///    `publish_component_meta_cache_entry`).
/// 2. The `ACTIVE_TRACERS` stack (also TLS), captured by the outer
///    `with_fact_tracer` scope in `component_meta_entry.rs` — used for
///    R20 overflow detection and (once the dual channels collapse) as
///    the canonical `fact_dep_signature` source.
///
/// Dual-emit is the safe migration substrate: both channels receive
/// the same dispatch facts so the curated signature retains coverage
/// today AND the `fact_dep_signature` source can later switch from
/// `state.fact_versions` to the tracer's `read_set.finalise()`
/// without losing a single fact. The fact-tracer fan-out alone will
/// suffice once the producer source flips to `read_set.finalise()`.
///
/// The function records two provenance counters
/// (`slot_binding_graph_fact_tracer_emissions` and
/// `slot_binding_graph_legacy_accumulator_emissions`) so tests can
/// discriminate the dual-emit invariant under unrelated / related
/// dep edits.
fn emit_slot_binding_graph_dispatch_facts(
    ctx: &dyn ResolverContext,
    sig: &crate::semantic_query::DepSignature,
) {
    use std::sync::atomic::Ordering::Relaxed;
    // Legacy: feed the per-request accumulator that drains into
    // `state.fact_versions`.
    accumulate_dispatch_dep_signature(sig);
    if let Some(prov) = ctx.project_type_store().semantic_graph().provenance() {
        prov.slot_binding_graph_legacy_accumulator_emissions
            .fetch_add(1, Relaxed);
    }

    // New: fan into the `ACTIVE_TRACERS` stack so the outer
    // `with_fact_tracer` captures the same facts. The bridge helper
    // converts `DepSignature` → `Vec<FactVersionRef>` (Block 0
    // helper #5); only `DepVersion::WholeHash` survives the
    // conversion — route-generation / project-generation entries are
    // R20-only signals and have no `FactVersionRef` equivalent.
    let bridged = crate::fact_signature_helpers::dep_signature_to_fact_signature(sig);
    crate::fact_signature_helpers::observe_fact_signature(&bridged);
    if let Some(prov) = ctx.project_type_store().semantic_graph().provenance() {
        prov.slot_binding_graph_fact_tracer_emissions
            .fetch_add(1, Relaxed);
    }
}
use crate::semantic_query::{
    DeclIdentity, DepSignature, DepVersion, PathSegment, ProjectionMode, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use crate::types::FileAnalysisSnapshot;

/// Identifies a single `defineSlots` (or peer macro) invocation by
/// `(owner_canonical, macro_index, type_args)`. Used as the primary key
/// for graph-native binding resolution so distinct invocations of the
/// same macro on the same owner do not alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SlotMacroIdentity {
    pub owner: DeclIdentity,
    pub macro_index: usize,
    pub type_args: Arc<[SemanticNodeId]>,
}

/// Source classification for a resolved binding row. Used by
/// [`publish_merged_bindings`] to decide whether parser-path metadata
/// (e.g. `raw_type`) should be merged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotBindingSource {
    /// Resolved by walking the macro payload's empty-path Shallow surface.
    GraphNative,
    /// Resolved from parser-side `AnalyzedSlotFieldBinding` (no graph
    /// contribution).
    #[allow(dead_code)] // surfaces in publish_merged_bindings parser-only branch
    ParserPath,
}

/// In-memory model for a graph-native slot binding row. Used as the
/// staging form before [`publish_merged_bindings`] writes the row into
/// `ExpandedComponentTypes::slot_bindings`.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedSlotBinding {
    /// Provenance back to the macro invocation that produced this row.
    /// Retained for diagnostic correlation and future per-macro audit
    /// payload emission; the publication merge does not currently
    /// consume it.
    #[allow(dead_code)]
    pub owner_macro: SlotMacroIdentity,
    pub slot_name: Arc<str>,
    pub binding_name: Arc<str>,
    /// Maps to the `value` field on `SurfaceMember`.
    pub value_node: SemanticNodeId,
    pub optional: bool,
    /// Captured from the surface member's TypeScript `readonly` modifier.
    /// `ExpandedField` does not yet expose a `readonly` channel; the
    /// field is staged so a follow-up can route it through publication
    /// without re-walking the graph.
    #[allow(dead_code)]
    pub readonly: bool,
    /// Discriminator between graph-native and parser-path-only bindings.
    /// `publish_merged_bindings` currently only iterates graph-native
    /// rows, but the field is retained for the parser-path-only branch
    /// added by a future merge mode.
    #[allow(dead_code)]
    pub source: SlotBindingSource,
}

/// Result of [`resolve_slot_bindings_graph_native`].
///
/// `should_suppress` is consumed by the caller and gates
/// `ComponentMetaResultDb` publication: when a fatal `QueryError`
/// propagated up through the per-macro walk, the partially-populated
/// result must not warm the shared cache.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SynthesisResult {
    pub should_suppress: bool,
}

/// Owner declaration sentinel used for SFC `<script setup>` macro
/// identities. Every macro invocation in an SFC's `<script setup>`
/// resolves under the same synthetic declaration name; downstream
/// dispatch only consults the canonical id + whole hash for cache
/// keying.
const SFC_SCRIPT_SETUP_DECL_NAME: &str = "<sfc-script-setup>";

/// Build the owner [`DeclIdentity`] for an SFC's macro queries.
fn build_owner_decl_identity(ctx: &dyn ResolverContext, owner_canonical: &str) -> DeclIdentity {
    let whole_hash = ctx
        .shallow_file_state(owner_canonical)
        .map(|s| s.whole_hash)
        .unwrap_or_default();
    DeclIdentity {
        canonical_id: Arc::from(owner_canonical),
        whole_hash,
        decl_name: Arc::from(SFC_SCRIPT_SETUP_DECL_NAME),
    }
}

/// Returns `true` when a [`QueryError`] should propagate as a fatal
/// suppression signal: budget breaches and unstable-state retries
/// must not warm the shared cache.
#[inline]
fn is_fatal_query_error(err: &QueryError) -> bool {
    matches!(
        err,
        QueryError::BudgetExceeded(_) | QueryError::UnstableState { .. }
    )
}

/// Build a `MacroExpansionDiagnostics` envelope describing a fatal
/// error encountered while reading a synthesis sub-query. Carries
/// the macro_kind so consumers can route the diagnostic to the
/// right surface.
fn macro_expansion_for_query_error(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::InstantiationError,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Interrupted,
    }
}

/// Build a `MacroExpansionDiagnostics` envelope describing a recursive
/// back-edge encountered while reading a synthesis sub-query. Cycles
/// are not fatal — they bound the publish surface to the
/// non-recursive arms — so the envelope publishes
/// `Completed`/`Incomplete` rather than `Interrupted`.
fn macro_expansion_for_cycle(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::CyclicReference,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Completed,
    }
}

/// Build a `MacroExpansionDiagnostics` envelope describing a synthesis
/// step-budget breach. Carries the macro_kind so consumers can route
/// the diagnostic to the right surface.
///
/// Emits `ExpansionStopReason::BudgetExceeded` with `Interrupted`
/// execution status: a budget-exceeded run is not a complete
/// synthesis, so callers (and downstream `should_suppress` consumers)
/// must treat the published surface as torn.
fn macro_expansion_for_budget_exceeded(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::BudgetExceeded,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Interrupted,
    }
}

/// Walk a freshly-lowered macro-arg [`SemanticNodeId`] and accumulate
/// `(canonical_id, WholeHash)` dep facts for every cross-file
/// `DeclRef` / `InstantiationRef` carrier whose canonical differs from
/// `owner_canonical`.
///
/// This closes the gap where the synthesis dispatch path (`ProjectPath`,
/// `ResolveMacroPayload`) routes the inner cross-file `ResolveDecl`
/// through the dep-signature-discarding dispatch API: the returned
/// [`DepSignature`] is collapsed to the project-generation token by
/// `build_project_path`. Without this carrier-side accumulator, an
/// `import type { Slots } from './types'` carrier file is loaded by the
/// shallow lowering / dispatch walk but its whole-hash never reaches
/// the per-request signature accumulator, so the published
/// `ComponentMetaResultDb` entry's `dep_signature` does not include the
/// carrier and an edit to the carrier does not invalidate the warm
/// cache through the dep-signature validator.
///
/// Carrier facts feed the same TLS accumulator that `execute_read`
/// reads merge into, so the existing drain in
/// `compute_component_meta_state_inner` picks them up before publish.
///
/// Cycle-safe: keeps a `visited` set of `SemanticNodeId`s so a self-
/// referential lowered shape (e.g. `type R = { next: R }` in Navigate
/// mode) terminates after the first visit.
fn accumulate_lowered_node_carrier_deps(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    owner_canonical: &str,
) {
    let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut carriers: FxHashMap<Arc<str>, [u8; 16]> = FxHashMap::default();
    let mut stack: Vec<SemanticNodeId> = vec![node];
    while let Some(current) = stack.pop() {
        if !visited.insert(current) {
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, current) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                if identity.canonical_id.as_ref() != owner_canonical {
                    carriers
                        .entry(Arc::clone(&identity.canonical_id))
                        .or_insert(identity.whole_hash);
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                if base.canonical_id.as_ref() != owner_canonical {
                    carriers
                        .entry(Arc::clone(&base.canonical_id))
                        .or_insert(base.whole_hash);
                }
                for arg in args.iter() {
                    stack.push(*arg);
                }
            }
            SemanticNodeData::Alias(inner) => {
                stack.push(*inner);
            }
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                for arm in arms.iter() {
                    stack.push(*arm);
                }
            }
            SemanticNodeData::Array { element, .. } => {
                stack.push(*element);
            }
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter() {
                    stack.push(element.value);
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for expr in expressions.iter() {
                    stack.push(*expr);
                }
            }
            SemanticNodeData::KeyOf { base } => {
                stack.push(*base);
            }
            SemanticNodeData::IndexedAccess { object, .. } => {
                stack.push(*object);
            }
            SemanticNodeData::Mapped { source, .. } => {
                stack.push(*source);
            }
            // Object / Function surface bodies, primitives, literals,
            // type-params, infer placeholders, opaques, typeof shells,
            // and Vue macro elements have no further carrier-bearing
            // children for the purpose of dep-signature carrier
            // discovery from the lowered macro arg. Object/Function
            // surfaces only appear here when they are inline structural
            // types in the SFC's own scope; nested cross-file refs
            // surface as `DeclRef` / `InstantiationRef` carriers and
            // are picked up directly when the walker reaches them.
            _ => {}
        }
    }
    if carriers.is_empty() {
        return;
    }
    let entries: Vec<(Arc<str>, DepVersion)> = carriers
        .into_iter()
        .map(|(canonical, hash)| (canonical, DepVersion::WholeHash(hash)))
        .collect();
    let signature: DepSignature = Arc::from(entries.into_boxed_slice());
    // Dual-emit: legacy accumulator + fact-tracer fan-out.
    emit_slot_binding_graph_dispatch_facts(ctx, &signature);
}

/// Read the [`SurfaceView`] members backing `node`, if `node` resolves
/// to a `SemanticNodeData::Object` shell. Empty for any other variant
/// — callers treat the empty surface as "no enumerable members".
fn read_surface_members(
    ctx: &dyn ResolverContext,
    surface_node: SemanticNodeId,
) -> Vec<crate::semantic_query::SurfaceMember> {
    match crate::project_semantic_dispatch::node_data_for(ctx, surface_node).as_deref() {
        Some(SemanticNodeData::Object(view)) => view.members.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Returns `true` when the slot-param's underlying shape is one that
/// the synthesis must NOT enumerate as a concrete object surface,
/// because the shape's binding identity depends on a generic /
/// indexed / mapped / conditional context that has not been resolved.
///
/// Walks through `Alias` and `InstantiationRef` (Skeleton-instantiated
/// body) hops to reach an effective root. Returns `true` for:
///
/// - `Conditional` (open or deferred — branch identity is undetermined
///   so neither branch's bindings are authoritative)
/// - `IndexedAccess` (e.g. `T["slot-name"]` — the indexed root has
///   not been resolved to a concrete shape)
/// - `Mapped`, `KeyOf`, `TemplateLiteral` (key-space transformations
///   with symbolic source)
/// - `TypeParam`, `Infer` (open generics)
///
/// Returns `false` for `Object`, `Function`, `Primitive`, `Literal`,
/// `Union`, `Intersection`, `Tuple`, `Array`, etc. — these are
/// directly enumerable by the empty-path Shallow walker.
///
/// Depth-fused at 256 to bound recursion on adversarial inputs.
fn slot_param_root_is_symbolic_only(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    depth: u32,
) -> bool {
    if depth > 256 {
        return false;
    }
    let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            slot_param_root_is_symbolic_only(dispatch, *inner, depth + 1)
        }
        SemanticNodeData::Conditional { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. } => true,
        SemanticNodeData::InstantiationRef { base, .. } => {
            // Resolve the body via Skeleton-instantiation so unbound
            // type parameters become TypeParam shells (preserving
            // Conditional branches that would otherwise collapse to
            // `never`).
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let key = SemanticQueryKey::Instantiate {
                base: base.clone(),
                args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                context: crate::semantic_query::ProjectionReductionContext::published(
                    ProjectionMode::Skeleton,
                ),
            };
            let read = dispatch.execute_read(key);
            // Dual-emit: legacy accumulator + fact-tracer fan-out.
            emit_slot_binding_graph_dispatch_facts(dispatch.ctx, &read.dep_signature);
            match read.value {
                QueryResult::Value(body_id) => {
                    slot_param_root_is_symbolic_only(dispatch, body_id, depth + 1)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Compute the exactness flag for a synthesised binding's value node.
///
/// Thin wrapper around the shared
/// [`crate::meta_resolve::exactness::classify_node`] predicate so the
/// slot-binding and `defineProps` paths stay in lockstep on the
/// alias-unwrap + closed-object semantics. See the `exactness` module
/// docs for the full predicate.
pub(crate) fn compute_exactness_for_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> ExpansionExactness {
    super::exactness::classify_node(dispatch, node)
}

/// Graph-native synthesis entry point for `defineSlots` slot-binding
/// extraction.
///
/// Resolves all `defineSlots` macros' bindings via empty-path Shallow
/// walks against the graph, merges with parser-path bindings, and
/// publishes each row to `expanded.slot_bindings`. Returns
/// [`SynthesisResult`] so the caller can gate `ComponentMetaResultDb`
/// publication on the suppression flag.
pub(crate) fn resolve_slot_bindings_graph_native(
    ctx: &mut ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    expanded: &mut ExpandedComponentTypes,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> SynthesisResult {
    let span = tracing::info_span!(
        "synthesize_slot_bindings",
        file = %owner_canonical,
        macro_count = snapshot.macros.len(),
    );
    let _enter = span.enter();
    // Synthesis-entry info event. Per the audit contract the synthesis layer
    // emits at `info` level; the `tracing::info!` here uses the
    // module-path target so subscribers filtered to `verter_session`
    // (e.g. tracing-test) capture the span name on its formatted
    // line. The span attributes are reflected into the event so log
    // consumers see `file=…` and `macro_count=…` in the same record.
    tracing::info!(
        file = %owner_canonical,
        macro_count = snapshot.macros.len(),
        "synthesize_slot_bindings",
    );

    let mut should_suppress = false;
    let dispatch = ProjectSemanticDispatch::new(ctx.ctx);
    let owner = build_owner_decl_identity(ctx.ctx, owner_canonical);

    // Synthesis-step budget. Production code leaves
    // `synthesis_steps` `None` so the synthesis runs at full
    // budget. Tests use a small override to drive the budget-
    // exceeded path on a hermetic fixture without requiring a
    // pathological corpus. Each synthesis sub-action (lower macro
    // arg, ResolveMacroPayload dispatch, slot-surface walk, per-
    // member param walk) increments the counter; when the counter
    // exceeds the cap, synthesis bails with
    // `ExpansionStopReason::BudgetExceeded` and marks the run for
    // suppression so the result is not promoted into the final
    // `ComponentMetaResultDb` cache.
    let synthesis_step_budget: Option<u32> =
        ctx.ctx.config().recursion_budget_overrides.synthesis_steps;
    let mut synthesis_steps_executed: u32 = 0;
    // Returns `true` when consuming a step exhausted the budget. The
    // closure increments the counter unconditionally so successive
    // calls observe progress; the `> cap` check fires once the
    // counter crosses the configured cap (matches the
    // `projection_op_count` semantics: cap=N permits N steps and
    // fires on the N+1-th).
    let consume_synthesis_step = |steps: &mut u32| -> bool {
        *steps = steps.saturating_add(1);
        match synthesis_step_budget {
            Some(cap) => *steps > cap,
            None => false,
        }
    };

    // First pass: graph-native synthesis. Build a map keyed by
    // `(slot_name, binding_name)`. Map dedups distinct macro
    // invocations that surface the same slot/binding combination.
    let mut graph_native_bindings: FxHashMap<(Arc<str>, Arc<str>), ResolvedSlotBinding> =
        FxHashMap::default();

    'macro_loop: for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if mac.kind != AnalyzedMacroKind::DefineSlots || !mac.is_type_based {
            continue;
        }
        let parsed_arg = match mac.parsed_type_argument.as_ref() {
            Some(t) => t,
            None => continue,
        };
        let macro_span = tracing::info_span!(
            "synthesize_macro",
            macro_index,
            kind = ?mac.kind,
        );
        let _enter_macro = macro_span.enter();
        // Per-macro info event. Same module-path target as the
        // outer synthesis-entry event so tracing-test captures the
        // span path on the formatted log line, satisfying §17.5
        // (synthesis layer emits at `info`).
        tracing::info!(
            macro_index,
            kind = ?mac.kind,
            "synthesize_macro",
        );

        // Step 1: lower macro arg via Navigate. Navigate keeps any
        // imported carrier types as lazy shells — the shallow walker
        // will materialise the surface when the dispatch reads it.
        if consume_synthesis_step(&mut synthesis_steps_executed) {
            should_suppress = true;
            diag_sink.push(macro_expansion_for_budget_exceeded(
                macro_index,
                MacroExpansionKind::DefineSlots,
                format!(
                    "synthesis-step-budget-exceeded@lower-macro-arg::steps={}::cap={:?}",
                    synthesis_steps_executed, synthesis_step_budget,
                ),
            ));
            break 'macro_loop;
        }
        let type_args: Arc<[SemanticNodeId]> = match dispatch.lower_type_expr_in_scope_with_mode(
            owner_canonical,
            parsed_arg,
            ProjectionMode::Navigate,
        ) {
            Some(node) => Arc::from(vec![node].into_boxed_slice()),
            None => continue,
        };
        // Carrier-fact propagation: walk the lowered macro arg and
        // accumulate `(canonical_id, WholeHash)` for any cross-file
        // `DeclRef` / `InstantiationRef` carrier so the per-request
        // dep-signature accumulator picks up imported carriers (e.g.
        // `import type { Slots } from './types'`). Without this, the
        // inner shallow walker's dep-signature-discarding `ResolveDecl`
        // dispatch path drops the carrier whole-hash, the
        // `ComponentMetaResultDb` entry is published with a
        // dep-signature missing the carrier, and an edit to the
        // carrier does not invalidate the warm cache through the
        // dep-signature validator.
        for arg in type_args.iter() {
            accumulate_lowered_node_carrier_deps(ctx.ctx, *arg, owner_canonical);
        }

        // Step 2: ResolveMacroPayload. USE execute_read; ACCUMULATE deps.
        if consume_synthesis_step(&mut synthesis_steps_executed) {
            should_suppress = true;
            diag_sink.push(macro_expansion_for_budget_exceeded(
                macro_index,
                MacroExpansionKind::DefineSlots,
                format!(
                    "synthesis-step-budget-exceeded@resolve-macro-payload::steps={}::cap={:?}",
                    synthesis_steps_executed, synthesis_step_budget,
                ),
            ));
            break 'macro_loop;
        }
        let macro_payload_read = dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload {
            owner: owner.clone(),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineSlots,
            type_args: type_args.clone(),
            mode: ProjectionMode::Navigate,
        });
        // Dual-emit: legacy accumulator + fact-tracer fan-out.
        emit_slot_binding_graph_dispatch_facts(dispatch.ctx, &macro_payload_read.dep_signature);
        if !macro_payload_read.walker_diagnostics.is_empty() {
            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                &macro_payload_read.walker_diagnostics,
                macro_index,
                MacroExpansionKind::DefineSlots,
                macro_payload_read.cache_suppress,
            ));
        }
        if macro_payload_read.cache_suppress {
            should_suppress = true;
        }

        let macro_payload_node = match macro_payload_read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) => {
                diag_sink.push(macro_expansion_for_cycle(
                    macro_index,
                    MacroExpansionKind::DefineSlots,
                    "cyclic-macro-payload@defineSlots".to_string(),
                ));
                continue;
            }
            QueryResult::Error(e) => {
                if is_fatal_query_error(&e) {
                    should_suppress = true;
                }
                diag_sink.push(macro_expansion_for_query_error(
                    macro_index,
                    MacroExpansionKind::DefineSlots,
                    format!("macro-payload-error::{:?}", e),
                ));
                continue;
            }
        };

        // Step 3: enumerate slots via empty-path Shallow.
        let bindings = compute_bindings_via_graph(
            &dispatch,
            ctx.ctx,
            macro_payload_node,
            SlotMacroIdentity {
                owner: owner.clone(),
                macro_index,
                type_args: type_args.clone(),
            },
            diag_sink,
            &mut should_suppress,
            synthesis_step_budget,
            &mut synthesis_steps_executed,
        );
        if should_suppress && synthesis_step_budget.is_some() {
            // Budget-exceeded inside `compute_bindings_via_graph`
            // already pushed the diagnostic and flipped suppression;
            // bail the macro loop so subsequent macros do not consume
            // additional steps under an already-exhausted budget.
            break 'macro_loop;
        }

        for binding in bindings {
            tracing::trace!(
                target: "verter::meta_resolve::slot_binding",
                slot = %binding.slot_name,
                binding = %binding.binding_name,
                "graph_native_binding_collected",
            );
            graph_native_bindings.insert(
                (binding.slot_name.clone(), binding.binding_name.clone()),
                binding,
            );
        }
    }

    // Second pass: merge with parser-path bindings, publishing each
    // row. The published seen-name set carries any parser-path
    // bindings already emitted by upstream stages so duplicates are
    // suppressed.
    let mut existing_names: FxHashSet<String> = expanded
        .slot_bindings
        .iter()
        .map(|b| b.name.clone())
        .collect();

    publish_merged_bindings(
        &dispatch,
        &graph_native_bindings,
        resolved_macros,
        expanded,
        &mut existing_names,
    );

    SynthesisResult { should_suppress }
}

/// Per-macro graph-native binding computation. Walks
/// `macro_payload_node`'s empty-path Shallow surface, then for each
/// slot member walks the `Function.params[0].ty`'s Shallow surface to
/// enumerate bindings.
///
/// Aggregates `should_suppress` via `&mut bool` so every fatal
/// `QueryError` propagates up to
/// [`resolve_slot_bindings_graph_native`]'s return.
///
/// The `synthesis_step_budget` / `synthesis_steps_executed` pair
/// extends the same step counter the entry-point owns so the slot-
/// surface walk and per-member param walk participate in the same
/// `synthesis_steps` cap. Returning early via the `BudgetExceeded`
/// branch sets `*should_suppress = true` so the caller skips
/// publication.
pub(crate) fn compute_bindings_via_graph(
    dispatch: &ProjectSemanticDispatch<'_>,
    ctx: &dyn ResolverContext,
    macro_payload_node: SemanticNodeId,
    owner_macro: SlotMacroIdentity,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    should_suppress: &mut bool,
    synthesis_step_budget: Option<u32>,
    synthesis_steps_executed: &mut u32,
) -> Vec<ResolvedSlotBinding> {
    let mut out = Vec::new();
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
    let consume_step = |steps: &mut u32| -> bool {
        *steps = steps.saturating_add(1);
        match synthesis_step_budget {
            Some(cap) => *steps > cap,
            None => false,
        }
    };

    // Step 3: empty-path Shallow surface for slot names.
    if consume_step(synthesis_steps_executed) {
        *should_suppress = true;
        diag_sink.push(macro_expansion_for_budget_exceeded(
            owner_macro.macro_index,
            MacroExpansionKind::DefineSlots,
            format!(
                "synthesis-step-budget-exceeded@slot-surface::steps={}::cap={:?}",
                *synthesis_steps_executed, synthesis_step_budget,
            ),
        ));
        return out;
    }
    let slot_surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: macro_payload_node,
        path: empty_path.clone(),
        mode: ProjectionMode::Shallow,
    });
    // Dual-emit: legacy accumulator + fact-tracer fan-out.
    emit_slot_binding_graph_dispatch_facts(ctx, &slot_surface_read.dep_signature);
    if !slot_surface_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &slot_surface_read.walker_diagnostics,
            owner_macro.macro_index,
            MacroExpansionKind::DefineSlots,
            slot_surface_read.cache_suppress,
        ));
    }
    if slot_surface_read.cache_suppress {
        *should_suppress = true;
    }
    let slot_surface = match slot_surface_read.value {
        QueryResult::Value(id) => id,
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                owner_macro.macro_index,
                MacroExpansionKind::DefineSlots,
                "cyclic-slot-surface".to_string(),
            ));
            return out;
        }
        QueryResult::Error(e) => {
            if is_fatal_query_error(&e) {
                *should_suppress = true;
            }
            diag_sink.push(macro_expansion_for_query_error(
                owner_macro.macro_index,
                MacroExpansionKind::DefineSlots,
                format!("slot-surface-error::{:?}", e),
            ));
            return out;
        }
    };
    let slot_members = read_surface_members(ctx, slot_surface);

    for slot_member in slot_members.iter() {
        // Step 4: read Function.params[0].ty.
        let param0_ty = match crate::project_semantic_dispatch::node_data_for(
            ctx,
            slot_member.value,
        )
        .as_deref()
        {
            Some(SemanticNodeData::Function { params, .. }) => match params.first() {
                Some(p) => p.ty,
                None => continue,
            },
            _ => continue,
        };

        // Skip slots whose binding parameter has a symbolic-only root
        // shape (Conditional/IndexedAccess/Mapped/KeyOf/TypeParam/etc.).
        // The empty-path Shallow walker would either commit to a single
        // branch (closed conditional) or materialise a Mapped surface;
        // both outcomes contradict the slot-binding contract, which
        // preserves the symbolic shape until a concrete callsite
        // resolves the generic context. The downstream parser-path
        // analysis still publishes a binding row when the source-text
        // annotation is concrete; the synthesis here just declines to
        // overwrite that row with a materialised guess.
        if slot_param_root_is_symbolic_only(dispatch, param0_ty, 0) {
            tracing::trace!(
                target: "verter::meta_resolve::slot_binding",
                slot = %slot_member.name,
                "graph_native_skip_symbolic_param_root",
            );
            continue;
        }

        // Empty-path Shallow on param0_ty.
        if consume_step(synthesis_steps_executed) {
            *should_suppress = true;
            diag_sink.push(macro_expansion_for_budget_exceeded(
                owner_macro.macro_index,
                MacroExpansionKind::DefineSlots,
                format!(
                    "synthesis-step-budget-exceeded@param-surface::slot={}::steps={}::cap={:?}",
                    slot_member.name, *synthesis_steps_executed, synthesis_step_budget,
                ),
            ));
            return out;
        }
        let param_surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
            base: param0_ty,
            path: empty_path.clone(),
            mode: ProjectionMode::Shallow,
        });
        // Dual-emit: legacy accumulator + fact-tracer fan-out.
        emit_slot_binding_graph_dispatch_facts(ctx, &param_surface_read.dep_signature);
        if !param_surface_read.walker_diagnostics.is_empty() {
            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                &param_surface_read.walker_diagnostics,
                owner_macro.macro_index,
                MacroExpansionKind::DefineSlots,
                param_surface_read.cache_suppress,
            ));
        }
        if param_surface_read.cache_suppress {
            *should_suppress = true;
        }
        let param_surface = match param_surface_read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) => {
                diag_sink.push(macro_expansion_for_cycle(
                    owner_macro.macro_index,
                    MacroExpansionKind::DefineSlots,
                    format!("cyclic-slot-param@{}", slot_member.name),
                ));
                continue;
            }
            QueryResult::Error(e) => {
                if is_fatal_query_error(&e) {
                    *should_suppress = true;
                }
                diag_sink.push(macro_expansion_for_query_error(
                    owner_macro.macro_index,
                    MacroExpansionKind::DefineSlots,
                    format!("slot-param-error::{}::{:?}", slot_member.name, e),
                ));
                continue;
            }
        };
        let binding_members = read_surface_members(ctx, param_surface);

        for binding in binding_members.iter() {
            out.push(ResolvedSlotBinding {
                owner_macro: owner_macro.clone(),
                slot_name: slot_member.name.clone(),
                binding_name: binding.name.clone(),
                value_node: binding.value,
                optional: binding.optional,
                readonly: binding.readonly,
                source: SlotBindingSource::GraphNative,
            });
        }
    }
    out
}

/// Merge graph-native bindings with parser-path bindings and publish
/// each row to `expanded.slot_bindings`.
///
/// `raw_type` policy: graph-native bindings get parser-path `raw_type`
/// if a parser-path binding with the same `(slot, binding)` name exists;
/// parser-path-only bindings are published with their parser-path
/// `raw_type` and `ExactConcrete` exactness.
pub(crate) fn publish_merged_bindings(
    dispatch: &ProjectSemanticDispatch<'_>,
    graph_native: &FxHashMap<(Arc<str>, Arc<str>), ResolvedSlotBinding>,
    resolved_macros: &[ResolvedMacroMeta],
    expanded: &mut ExpandedComponentTypes,
    existing_names: &mut FxHashSet<String>,
) {
    // Index parser-path bindings by `(slot_name, binding_name)`.
    let mut parser_index: FxHashMap<
        (Arc<str>, Arc<str>),
        &verter_semantic::analysis::AnalyzedSlotFieldBinding,
    > = FxHashMap::default();
    for resolved in resolved_macros
        .iter()
        .filter(|r| r.macro_kind == AnalyzedMacroKind::DefineSlots)
    {
        for slot in &resolved.slots {
            for binding in &slot.bindings {
                parser_index.insert(
                    (
                        Arc::from(slot.name.as_str()),
                        Arc::from(binding.name.as_str()),
                    ),
                    binding,
                );
            }
        }
    }

    // Publish each graph-native binding, merging parser-path metadata
    // when present.
    let mut graph_native_keys: Vec<(Arc<str>, Arc<str>)> = graph_native.keys().cloned().collect();
    graph_native_keys.sort();
    for key in graph_native_keys {
        let gb = match graph_native.get(&key) {
            Some(gb) => gb,
            None => continue,
        };
        let (slot_name, binding_name) = key;
        let field_name = format!("{}.{}", slot_name, binding_name);
        if !existing_names.insert(field_name.clone()) {
            continue;
        }

        let r#type = dispatch
            .raise_node_to_type_expr(gb.value_node)
            .unwrap_or(TypeExpr::Unknown {
                raw: "semanticMiss".to_string(),
            });
        let exactness = compute_exactness_for_node(dispatch, gb.value_node);

        // Merge parser-path metadata: ONLY raw_type — description /
        // tags do not live on `ExpandedField`.
        let parser_path = parser_index.remove(&(slot_name.clone(), binding_name.clone()));
        let raw_type = parser_path.and_then(|p| p.type_annotation.clone());

        tracing::trace!(
            target: "verter::meta_resolve::slot_binding",
            field_name = %field_name,
            exactness = ?exactness,
            has_raw_type = raw_type.is_some(),
            "publish_slot_binding",
        );

        expanded.slot_bindings.push(ExpandedField {
            name: field_name,
            r#type,
            raw_type,
            optional: gb.optional,
            exactness,
            execution_status: ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
        });
    }

    // Publish parser-path-only bindings (those without a graph-native
    // counterpart). Parser-path bindings keep `ExactConcrete`
    // exactness — the source-text annotation is the authority.
    let mut parser_only_keys: Vec<(Arc<str>, Arc<str>)> = parser_index.keys().cloned().collect();
    parser_only_keys.sort();
    for key in parser_only_keys {
        let pb = match parser_index.get(&key) {
            Some(pb) => *pb,
            None => continue,
        };
        let (slot_name, binding_name) = key;
        let field_name = format!("{}.{}", slot_name, binding_name);
        if !existing_names.insert(field_name.clone()) {
            continue;
        }
        let raw_type = pb.type_annotation.clone();
        // Typed-IR-Only Resolver Rule: `binding.binding_expr` is the
        // authoritative typed form populated by the analyzer at OXC
        // visit time. W1.1c closed the producer gap for inline slot
        // bindings. No reparse of `type_annotation`.
        let shallow_type_expr = pb.binding_expr.clone();
        let shallow_type_expr_scope = pb.binding_expr_scope.clone();
        debug_assert_eq!(
            shallow_type_expr.is_some(),
            shallow_type_expr_scope.is_some(),
            "ExpandedField (parser-only slot binding) shallow_type_expr/shallow_type_expr_scope pairing violated for binding `{}`",
            field_name
        );
        let parsed_type = shallow_type_expr.clone().unwrap_or(TypeExpr::Unknown {
            raw: "unknown".to_string(),
        });
        expanded.slot_bindings.push(ExpandedField {
            name: field_name,
            r#type: parsed_type,
            raw_type,
            optional: false,
            exactness: ExpansionExactness::ExactConcrete,
            execution_status: ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr,
            shallow_type_expr_scope,
        });
    }
}
