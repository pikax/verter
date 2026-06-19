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

    // Fan into the `ACTIVE_TRACERS` stack so the outer
    // `with_fact_tracer` captures the same facts. The bridge helper
    // `dep_signature_to_fact_signature` converts
    // `DepSignature` → `Vec<FactVersionRef>`; only
    // `DepVersion::WholeHash` survives the conversion —
    // route-generation / project-generation entries are R20-only
    // signals and have no `FactVersionRef` equivalent.
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

/// Declaration-ordered (slot_name, binding_name) → resolved binding
/// staging type for the graph-native synthesis pass. A `Vec` preserves
/// the synthesis-time iteration order (declaration order of the slot's
/// function parameter object) through to `publish_merged_bindings`,
/// which walks the slice without sorting.
type GraphNativeBindingEntry = ((Arc<str>, Arc<str>), ResolvedSlotBinding);

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
            // Carrier `type_args` ARE descended via the shared accessor: a
            // `BareRef` / `TypeOf` / `ImportType` carrier applies its arguments
            // at the reference site, and an arg can carry a cross-file
            // `DeclRef` / `InstantiationRef` whose declaring file is a dep. The
            // carrier head is not resolved here (args-only).
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::ImportType(_) => {
                for arg in data.carrier_type_args().iter() {
                    stack.push(*arg);
                }
            }
            // Object / Function surface bodies, primitives, literals,
            // type-params, infer placeholders, and Vue macro elements have no
            // further carrier-bearing children for the purpose of dep-signature
            // carrier discovery from the lowered macro arg. Object/Function
            // surfaces only appear here when they are inline structural types in
            // the SFC's own scope; nested cross-file refs surface as `DeclRef` /
            // `InstantiationRef` carriers and are picked up directly when the
            // walker reaches them.
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
///
/// `pub(crate)` so the DTO slot-binding extractor
/// (`typeinfo::framework_surface::vue_exec::binding_fields_from_param_ty` via
/// `navigate_param_to_object_surface`) can apply the SAME open-vs-concrete gate
/// before materialising a slot-param object surface — otherwise an open generic
/// slot param (`SlotProps<M>` in a `generic="M"` component) would reduce to a
/// committed branch and the DTO path would invent a phantom binding that the
/// graph-native path correctly declined.
pub(crate) fn slot_param_root_is_symbolic_only(
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
        // A Conditional is symbolic-only ONLY when it is genuinely OPEN — i.e.
        // its CHECK still contains a free `TypeParam` / `Infer` shell, so the
        // branch identity is undetermined. The distinction:
        //
        // - FREE generic (`[M] extends ['hover']` in a `generic="M extends Mode"`
        //   component): the check carries the free `TypeParam` `M`, so the
        //   conditional is OPEN. Neither branch's bindings are authoritative —
        //   classify symbolic. (A `Published(Shallow)` reduction WOULD commit to
        //   a branch here via `M`'s constraint, inventing a phantom binding —
        //   which is exactly the bug this guard prevents.)
        // - CONCRETE substitution (`{ id: string } extends { id: infer U }`): the
        //   check is fully concrete (no free `TypeParam`), so the conditional is
        //   DECIDABLE. Reduce it through an empty-path `Published(Shallow)`
        //   `ProjectPath` (the projection walker applies inference-binding +
        //   decidable-conditional reduction that a bare `SemanticQueryKey::
        //   Conditional` dispatch leaves deferred for an `infer`-bearing extends
        //   clause) and classify the reduced terminal.
        SemanticNodeData::Conditional { check, .. } => {
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let check = *check;
            drop(data);
            // Open check (free TypeParam / Infer) → genuinely symbolic.
            if node_contains_free_type_param(dispatch, check, 0) {
                return true;
            }
            // Concrete check → reduce the conditional and classify the result.
            let empty_path: Arc<[crate::semantic_query::PathSegment]> =
                Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice());
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: node,
                path: empty_path,
                context: crate::semantic_query::ProjectionReductionContext::published(
                    ProjectionMode::Shallow,
                ),
            });
            // Dual-emit: legacy accumulator + fact-tracer fan-out.
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_slot_binding_graph_dispatch_facts(dispatch.ctx, &read.dep_signature);
            match read.value {
                // Decidable: reduced to a concrete terminal — classify it (a
                // concrete branch root is enumerable, not symbolic).
                QueryResult::Value(reduced) if reduced != node => {
                    slot_param_root_is_symbolic_only(dispatch, reduced, depth + 1)
                }
                // Stayed the deferred conditional shell (open / undecidable) —
                // genuinely symbolic-only.
                _ => true,
            }
        }
        SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. } => true,
        SemanticNodeData::InstantiationRef { base, args } => {
            // Skeleton-instantiate the carrier under its OWN `(base, args)` so the
            // substitution actually binds — but unbound parameters become
            // `TypeParam` SHELLS, never their declared DEFAULT. This is the
            // distinction the generic slot-param gate turns on:
            //
            // - CONCRETE substitution (`SlotProps<{ id: string }>`): the args
            //   bind, so the body's Conditional / Mapped check is concrete and
            //   the Conditional arm below reduces it to a concrete root (bindings
            //   enumerated). The earlier EMPTY-args instantiation erased the
            //   substitution and left every such body an OPEN Conditional, so the
            //   rows were silently dropped.
            // - FREE generic (`SlotProps<M>` in a `generic="M extends Mode"`
            //   component): `M` stays a `TypeParam` shell (Skeleton does NOT
            //   apply the `= Mode` default), so the body's Conditional check is
            //   open and the recursion classifies it symbolic — declining to
            //   invent bindings from an undetermined generic context. (A
            //   `Published(Shallow)` projection of the carrier WOULD apply the
            //   default and wrongly commit to a branch — hence Skeleton, not
            //   ProjectPath, on the carrier.)
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let key = SemanticQueryKey::Instantiate {
                base: dispatch
                    .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name)),
                args: Arc::clone(args),
                context: dispatch.instantiate_context_for(
                    &base.canonical_id,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Skeleton,
                    ),
                ),
            };
            let read = dispatch.execute_read(key);
            // Dual-emit: legacy accumulator + fact-tracer fan-out.
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_slot_binding_graph_dispatch_facts(dispatch.ctx, &read.dep_signature);
            match read.value {
                QueryResult::Value(body_id) if body_id != node => {
                    slot_param_root_is_symbolic_only(dispatch, body_id, depth + 1)
                }
                // Did not instantiate past the carrier shell (or errored) —
                // an unresolvable carrier has no concrete enumerable root.
                _ => true,
            }
        }
        _ => false,
    }
}

/// Returns `true` when `node`'s structure still contains a FREE `TypeParam` or
/// `Infer` shell — i.e. the type is NOT fully concrete. Used by the slot-param
/// gate to decide whether a Conditional's check is decidable: a check carrying
/// a free type parameter (`[M] extends ['hover']` in a generic component) is
/// OPEN and must stay symbolic, while a fully-concrete check
/// (`{ id: string } extends { id: infer U }`) is decidable and may reduce.
///
/// Walks the compound shapes a substituted check can take (Tuple / Array /
/// Union / Intersection / Object members / KeyOf / IndexedAccess), resolving
/// one-level `Alias` hops, and descends the `type_args` of the structural
/// carriers (`BareRef` / `TypeOf` / `ImportType`) — a free `TypeParam` inside a
/// carrier's applied arguments keeps the check open. `Infer` shells inside the
/// conditional's EXTENDS clause are intentionally NOT inspected here (this
/// predicate is applied to the CHECK only) — an `infer U` in `extends` is the
/// binding mechanism, not an open check parameter. The lazy declaration carriers
/// (`DeclRef` / `InstantiationRef`) are treated as NOT-free (they are concrete
/// declaration references, resolved elsewhere). Depth-fused at 256.
fn node_contains_free_type_param(
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
        SemanticNodeData::TypeParam { .. } | SemanticNodeData::Infer { .. } => true,
        SemanticNodeData::Alias(inner) => {
            node_contains_free_type_param(dispatch, *inner, depth + 1)
        }
        SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => members
            .iter()
            .any(|m| node_contains_free_type_param(dispatch, *m, depth + 1)),
        SemanticNodeData::Array { element, .. } => {
            node_contains_free_type_param(dispatch, *element, depth + 1)
        }
        SemanticNodeData::Tuple { elements, .. } => elements
            .iter()
            .any(|e| node_contains_free_type_param(dispatch, e.value, depth + 1)),
        SemanticNodeData::Object(view) => view
            .members
            .iter()
            .any(|m| node_contains_free_type_param(dispatch, m.value, depth + 1)),
        SemanticNodeData::KeyOf { base } => {
            node_contains_free_type_param(dispatch, *base, depth + 1)
        }
        SemanticNodeData::IndexedAccess { object, .. } => {
            node_contains_free_type_param(dispatch, *object, depth + 1)
        }
        // A `BareRef` / `TypeOf` / `ImportType` carrier applies its arguments at
        // the reference site; a free `TypeParam` inside those args makes the
        // node contain a free param (the check stays open). Descend the args via
        // the shared accessor (args-only; the carrier head is not resolved).
        SemanticNodeData::BareRef(_)
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::ImportType(_) => data
            .carrier_type_args()
            .iter()
            .any(|&a| node_contains_free_type_param(dispatch, a, depth + 1)),
        // Primitives, literals, functions, opaque, etc. carry no free open
        // parameter on the check path.
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
    // Scope the slot-binding synthesis phase on the active request context
    // so any `Instantiate { Expanded }` dispatched WHILE this guard is held
    // is attributed to synthesis (bumping
    // `RequestContext::synthesis_expanded_instantiate_calls`). The eagerness
    // guard asserts that synthesis-scoped count is zero — synthesis drives
    // the carrier walk in Navigate / Skeleton, never Expanded.
    let _synthesis_scope = crate::request_context::SynthesisScopeGuard::enter();
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

    // First pass: graph-native synthesis. Accumulate bindings in
    // declaration order — `compute_bindings_via_graph` walks slot
    // members and binding-row members in surface order, which is the
    // declaration order of the slot's function parameter object. A Vec
    // preserves that order through to `publish_merged_bindings`; the
    // (slot_name, binding_name) tuple is the dedup key for distinct
    // macro invocations that surface the same slot/binding combination
    // (the dedup is rare in practice — a single `defineSlots<T>()`
    // typically owns the slot surface).
    let mut graph_native_bindings: Vec<GraphNativeBindingEntry> = Vec::new();

    'macro_loop: for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if mac.kind != AnalyzedMacroKind::DefineSlots || !mac.is_type_based {
            continue;
        }
        // Presence guard: skip a `defineSlots` macro with no type argument.
        // The argument itself is NOT lowered here — the mirror handle
        // (`macro_type_arg_hot_ref` below) is the ONE producer — so the
        // presence check does not bind the value.
        if mac.parsed_type_argument.is_none() {
            continue;
        }
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

        // Step 1: read the macro arg's mode-neutral mirror handle (the ONE
        // producer). The carrier stays a lazy shell — the shallow walker
        // materialises the surface when the dispatch reads it downstream.
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
        let type_args: Arc<[SemanticNodeId]> = match crate::macro_hot_mirror::macro_type_arg_hot_ref(
            ctx.ctx,
            owner_canonical,
            macro_index,
        ) {
            Some(handle) => Arc::from(vec![handle.node()].into_boxed_slice()),
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
            owner: dispatch.type_slot_for(
                Arc::clone(&owner.canonical_id),
                Arc::clone(&owner.decl_name),
            ),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineSlots,
            type_args: type_args.clone(),
            context: dispatch
                .macro_payload_context_for(&owner.canonical_id, ProjectionMode::Navigate),
        });
        // Dual-emit: legacy accumulator + fact-tracer fan-out.
        crate::request_context::observe_component_meta_read_suppress(&macro_payload_read);
        emit_slot_binding_graph_dispatch_facts(dispatch.ctx, &macro_payload_read.dep_signature);
        if !macro_payload_read.walker_diagnostics.is_empty() {
            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                &macro_payload_read.walker_diagnostics,
                macro_index,
                MacroExpansionKind::DefineSlots,
                macro_payload_read.cache_suppress,
            ));
        }
        // A2 signal split: the component-meta warm gate keys on the
        // PARTIAL signal, not on inner-memo non-cacheability. A benign
        // non-cacheable nested read must NOT suppress a complete result.
        if macro_payload_read.result_is_partial {
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
            let key = (binding.slot_name.clone(), binding.binding_name.clone());
            // Linear-scan dedup: prefer the earlier-arrival binding
            // (declaration order takes precedence). Distinct macro
            // invocations rarely collide on the same (slot, binding);
            // when they do, the FIRST observation wins to preserve the
            // declaration-order rule.
            if graph_native_bindings
                .iter()
                .all(|(existing, _)| existing != &key)
            {
                graph_native_bindings.push((key, binding));
            }
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
        ctx.ctx,
        owner_canonical,
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    // Dual-emit: legacy accumulator + fact-tracer fan-out.
    crate::request_context::observe_component_meta_read_suppress(&slot_surface_read);
    emit_slot_binding_graph_dispatch_facts(ctx, &slot_surface_read.dep_signature);
    if !slot_surface_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &slot_surface_read.walker_diagnostics,
            owner_macro.macro_index,
            MacroExpansionKind::DefineSlots,
            slot_surface_read.cache_suppress,
        ));
    }
    // A2 signal split: key the warm gate on the PARTIAL signal only.
    if slot_surface_read.result_is_partial {
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
        // Public-only publication: a `private` / `protected` class member
        // recorded on the shared surface must NOT be published as a slot.
        if !slot_member.visibility.is_public() {
            continue;
        }
        // Realize the slot member value through the callable-
        // realization substrate before the `Function`-arm match.
        // Under transit-shallow macro publication the slot value may
        // carry a non-Function shell (Alias / Conditional /
        // InstantiationRef / DeclRef carriers) that the publication
        // terminal `Published(Shallow)` chose not to reduce.
        // [`crate::meta_resolve::dispatch_helpers::realize_callable_member`]
        // normalises through the carrier chain (relation-engine
        // Conditional reduction, transit-mode Instantiate,
        // ResolveDecl unwrap) so a decidable callable surfaces as a
        // `Function` node; non-callable shapes (Object / Union /
        // Intersection / Mapped / KeyOf / primitives) return `None`
        // and skip naturally below.
        let realized = crate::meta_resolve::dispatch_helpers::realize_callable_member(
            dispatch,
            slot_member.value,
            crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
        .unwrap_or(slot_member.value);

        // Step 4: read Function.params[0].ty.
        let param0_ty =
            match crate::project_semantic_dispatch::node_data_for(ctx, realized).as_deref() {
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
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Shallow,
            ),
        });
        // Dual-emit: legacy accumulator + fact-tracer fan-out.
        crate::request_context::observe_component_meta_read_suppress(&param_surface_read);
        emit_slot_binding_graph_dispatch_facts(ctx, &param_surface_read.dep_signature);
        if !param_surface_read.walker_diagnostics.is_empty() {
            diag_sink.push(shallow_diagnostics_to_macro_expansion(
                &param_surface_read.walker_diagnostics,
                owner_macro.macro_index,
                MacroExpansionKind::DefineSlots,
                param_surface_read.cache_suppress,
            ));
        }
        // A2 signal split: key the warm gate on the PARTIAL signal only.
        if param_surface_read.result_is_partial {
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
            // Public-only publication: a navigated class param's `private` /
            // `protected` member must NOT leak as a published slot binding.
            if !binding.visibility.is_public() {
                continue;
            }
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

/// Resolve a `.vue` macro's normalized component-meta DTOs through the shared
/// typeinfo Vue surface path (`vue_macro_dtos_with_ctx`, FullMetadata) — the
/// SOLE props/emits/slots/exposed authority.
///
/// `owner_canonical` is the SFC the macro CALL lives in; `macro_index` indexes
/// that SFC's `FileAnalysisSnapshot::macros`. The host-cached, request-validated
/// surface path re-derives `whole_hash` + `macro_kind` from the live (overlay-
/// aware) snapshot, so `root_identity` here is only a hint (the real key is
/// validated inside). The returned bundle populates exactly the field matching
/// `macro_kind` (`props` for `DefineProps` / `DefineModel`, `emits` for
/// `DefineEmits`, `slots` for `DefineSlots`, `exposed` for `DefineExpose`);
/// the others stay empty.
///
/// EVERY view-sensitive read flows through `ctx`: routing through
/// `vue_macro_dtos_with_ctx` (NOT the base-view `VerterHost::vue_macro_dtos`)
/// means an overlay session resolves the macro surface against its OVERLAY
/// content — a `publish_merged_bindings` slot read in an overlay session no
/// longer leaks the base host's slot bindings.
fn typeinfo_macro_dtos(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
) -> std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos> {
    let root_identity = ctx.get_whole_hash(owner_canonical).unwrap_or([0u8; 16]);
    let read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        ctx,
        &crate::typeinfo::types::VueMacroSurfaceRequest {
            owner_canonical: std::sync::Arc::from(owner_canonical),
            macro_index,
            macro_kind,
            root_identity,
            level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
        },
    );
    // Fold a genuine partial surface into the request-result completeness so
    // the enclosing component-meta result's warm promotion is refused.
    read.observe_partial();
    read.dtos
}

pub(crate) fn publish_merged_bindings(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    dispatch: &ProjectSemanticDispatch<'_>,
    graph_native: &[GraphNativeBindingEntry],
    resolved_macros: &[ResolvedMacroMeta],
    expanded: &mut ExpandedComponentTypes,
    existing_names: &mut FxHashSet<String>,
) {
    // Materialise each `defineSlots` macro's slot member set through the
    // typeinfo Vue surface (`vue_macro_dtos`, FullMetadata) -- the sole slots
    // authority (function-like members + first-param binding extraction).
    // Keyed on `(owner, resolved.macro_index, DefineSlots)`. The owned vector
    // outlives the borrowed `parser_index` below.
    let slot_field_sets: Vec<Vec<verter_semantic::analysis::AnalyzedSlotField>> = resolved_macros
        .iter()
        .filter(|r| r.macro_kind == AnalyzedMacroKind::DefineSlots)
        .map(|resolved| {
            typeinfo_macro_dtos(
                ctx,
                owner_canonical,
                resolved.macro_index,
                AnalyzedMacroKind::DefineSlots,
            )
            .slot_fields()
            .to_vec()
        })
        .collect();

    // Index parser-path bindings by `(slot_name, binding_name)`.
    let mut parser_index: FxHashMap<
        (Arc<str>, Arc<str>),
        &verter_semantic::analysis::AnalyzedSlotFieldBinding,
    > = FxHashMap::default();
    for slots in &slot_field_sets {
        for slot in slots {
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

    // Publish each graph-native binding in declaration order, merging
    // parser-path metadata when present. The slice walk preserves the
    // insertion order set by `compute_bindings_via_graph` — declaration
    // order of the slot's function parameter object. No alphabetic
    // sort here (the previous FxHashMap-keyed iteration needed a sort
    // for determinism; the Vec source is already deterministic).
    //
    // Shallow-publication invariant ([[component-meta-shallow-by-default-rule]],
    // [[shallow-first-class-universal-cache]]): the published `r#type`
    // MUST be a shallow carrier. `dispatch.raise_node_to_type_expr` is a
    // structural raise (recursive for
    // Object/Function/Array/Union/Intersection/Tuple) — calling it at a
    // publication boundary unfolds the whole structural tree whenever
    // an upstream `Published(Shallow)` reduction materialised a
    // binding's value node as a concrete shape (e.g. the IndexedAccess
    // `OwnProps['actions']` reducing into its underlying array body),
    // dragging in deeply-nested external members. The discriminating
    // regression at
    // `tests/cases/g_misc0/slot_binding_shallow_publication_tests.rs` characterises
    // the carrier-vs-expansion boundary.
    //
    // The producer-shallow contract here is variant-dispatched on the
    // availability of a parser-path binding for the `(slot, binding)`
    // key:
    //
    //   - Parser path available: the OXC-lowered `binding_expr` is
    //     the syntactic authority, carrying the source-text annotation
    //     verbatim (e.g. `IndexedAccess<Ref(OwnProps), 'actions'>`).
    //     The published `r#type` is that lowered expression unchanged;
    //     NO synthetic carrier is minted.
    //
    //   - No parser path: publish a
    //     `TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
    //       scope_canonical_id, surface_kind, slot_name, binding_name,
    //       value_node }))` carrier. The variant identity is the full
    //     tuple — intrinsic and structurally distinct from any real
    //     workspace alias. The carrier is shallow by construction; the
    //     `binding_name` is NOT a registry-lookup target.
    //
    // Downstream consumers (`reduce_published_field_types`,
    // `collect_component_meta_registry_public_field_refs`,
    // JS compat `compatSlotBindingTypeText`, audit footprint miner)
    // pre-empt on the variant identity directly — no sidecar table or
    // verdict cache exists. A consumer that needs to deepen the
    // carrier into its underlying member shape routes through
    // `ShapeCacheKey::semantic_node_whole(scope, SemanticNodeId(key.value_node), mode)`
    // — the same identity used by any regular member-shape route, and
    // the only legitimate explicit-deepen path (enforced by the
    // `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
    // architecture guard).
    for (key, gb) in graph_native.iter() {
        let (slot_name, binding_name) = key.clone();
        let field_name = format!("{}.{}", slot_name, binding_name);
        if !existing_names.insert(field_name.clone()) {
            continue;
        }

        // Consult the parser-path index BEFORE deciding publication
        // shape — the parser-lowered annotation is the syntactic
        // truth. Removing here also drives the parser-only fallback
        // loop below (which iterates the residual).
        let parser_path = parser_index.remove(&(slot_name.clone(), binding_name.clone()));

        let (r#type, shallow_type_expr, shallow_type_expr_scope, is_synthetic) = match parser_path
            .and_then(|pb| pb.binding_expr.as_ref().zip(pb.binding_expr_scope.as_ref()))
        {
            Some((expr, scope)) => {
                // Parser-path branch — the OXC-lowered annotation
                // is concrete; no synthetic carrier minted.
                let expr_owned = expr.clone();
                (
                    expr_owned.clone(),
                    Some(expr_owned),
                    Some(scope.clone()),
                    false,
                )
            }
            None => {
                // No-parser branch — mint the typed-IR synthetic
                // carrier variant. The carrier's identity is the
                // FULL `(scope_canonical_id, surface_kind,
                // slot_name, binding_name, value_node)` tuple —
                // intrinsic and structurally distinct from any
                // real workspace alias. The scope is the owning
                // macro's canonical id; the value-node is
                // `gb.value_node` (the `SemanticNodeId` the graph
                // publisher minted the carrier from).
                let scope_canonical: Arc<str> =
                    Arc::from(gb.owner_macro.owner.canonical_id.as_ref());
                let carrier_key = Arc::new(verter_type_expr::SyntheticCarrierKey {
                    scope_canonical_id: scope_canonical.clone(),
                    surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
                    slot_name: Some(slot_name.clone()),
                    binding_name: binding_name.clone(),
                    value_node: gb.value_node.0,
                });
                let carrier = TypeExpr::SyntheticSlotBinding(carrier_key);
                let scope = verter_type_expr::TypeExprScope::new(scope_canonical.as_ref());
                (carrier.clone(), Some(carrier), Some(scope), true)
            }
        };

        let exactness = compute_exactness_for_node(dispatch, gb.value_node);
        let raw_type = parser_path.and_then(|p| p.type_annotation.clone());

        debug_assert_eq!(
            shallow_type_expr.is_some(),
            shallow_type_expr_scope.is_some(),
            "ExpandedField (graph-native slot binding) shallow_type_expr/shallow_type_expr_scope pairing violated for binding `{}`",
            field_name
        );

        tracing::trace!(
            target: "verter::meta_resolve::slot_binding",
            field_name = %field_name,
            exactness = ?exactness,
            has_raw_type = raw_type.is_some(),
            has_parser_typed = parser_path.is_some(),
            is_synthetic_carrier = is_synthetic,
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
            shallow_type_expr,
            shallow_type_expr_scope,
            // Slot bindings are positional parameters of a slot's
            // function signature, not declared members of the macro
            // T's own body. The fact applies at the slot level (the
            // slot's name in `defineSlots<T>`'s T), not the binding
            // level — `false` is the structural truth.
            declared_in_macro_type_arg: false,
        });
    }

    // Publish parser-path-only bindings (those without a graph-native
    // counterpart). Parser-path bindings keep `ExactConcrete`
    // exactness — the source-text annotation is the authority.
    //
    // Parser-path-only bindings NEVER mint a synthetic carrier —
    // their `binding_expr` is the OXC-lowered authoritative form, not
    // a symbolic stand-in. Their published `r#type` is therefore
    // never a `TypeExpr::SyntheticSlotBinding` variant.
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
            // Slot bindings are positional parameters of a slot's
            // function signature, not declared members of the macro
            // T's own body — `false` is the structural truth (see
            // companion comment in `graph_native` push above).
            declared_in_macro_type_arg: false,
        });
    }
}

#[cfg(test)]
#[path = "slot_binding_graph_carrier_tests.rs"]
mod carrier_descent_tests;
