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
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_expr_lower::parse_type_annotation;
use verter_semantic::analysis::AnalyzedMacroKind;

use super::dep_signature::accumulate_dispatch_dep_signature;
use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::component_meta::ResolvedMacroMeta;
use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
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
/// Depth-fused at 256 to mirror `slot_binding_param_can_stay_symbolic_node`.
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
            // `never`). This mirrors the Skeleton-dispatch contract
            // used by `slot_binding_param_can_stay_symbolic_node`.
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let key = SemanticQueryKey::Instantiate {
                base: base.clone(),
                args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                body_mode: ProjectionMode::Skeleton,
            };
            let read = dispatch.execute_read(key);
            super::dep_signature::accumulate_dispatch_dep_signature(&read.dep_signature);
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

    // First pass: graph-native synthesis. Build a map keyed by
    // `(slot_name, binding_name)`. Map dedups distinct macro
    // invocations that surface the same slot/binding combination.
    let mut graph_native_bindings: FxHashMap<(Arc<str>, Arc<str>), ResolvedSlotBinding> =
        FxHashMap::default();

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
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
        let type_args: Arc<[SemanticNodeId]> = match dispatch.lower_type_expr_in_scope_with_mode(
            owner_canonical,
            parsed_arg,
            ProjectionMode::Navigate,
        ) {
            Some(node) => Arc::from(vec![node].into_boxed_slice()),
            None => continue,
        };

        // Step 2: ResolveMacroPayload. USE execute_read; ACCUMULATE deps.
        let macro_payload_read = dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload {
            owner: owner.clone(),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineSlots,
            type_args: type_args.clone(),
            mode: ProjectionMode::Navigate,
        });
        accumulate_dispatch_dep_signature(&macro_payload_read.dep_signature);
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
        );

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
pub(crate) fn compute_bindings_via_graph(
    dispatch: &ProjectSemanticDispatch<'_>,
    ctx: &dyn ResolverContext,
    macro_payload_node: SemanticNodeId,
    owner_macro: SlotMacroIdentity,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    should_suppress: &mut bool,
) -> Vec<ResolvedSlotBinding> {
    let mut out = Vec::new();
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::<PathSegment>::new().into_boxed_slice());

    // Step 3: empty-path Shallow surface for slot names.
    let slot_surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: macro_payload_node,
        path: empty_path.clone(),
        mode: ProjectionMode::Shallow,
    });
    accumulate_dispatch_dep_signature(&slot_surface_read.dep_signature);
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
        let param_surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
            base: param0_ty,
            path: empty_path.clone(),
            mode: ProjectionMode::Shallow,
        });
        accumulate_dispatch_dep_signature(&param_surface_read.dep_signature);
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
        let parsed_type = raw_type
            .as_deref()
            .map(parse_type_annotation)
            .unwrap_or_else(|| TypeExpr::Unknown {
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
        });
    }
}
