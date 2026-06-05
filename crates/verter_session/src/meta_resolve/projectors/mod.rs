//! Per-macro projectors for component-meta extraction.
//!
//! Each projector resolves a single macro's surface members through
//! the shared dispatch primitives (`SemanticQueryKey::ResolveMacroPayload`
//! followed by an empty-path `ProjectPath` in `Shallow` mode), raises
//! members to `TypeExpr`, and produces `Vec<ExpandedField>` (or the
//! macro-specific equivalent).
//!
//! Authority chain:
//!
//! 1. `dispatch.lower_type_expr_in_scope_with_mode(file, parsed_arg, Navigate)`
//!    lowers the parsed type argument to a `SemanticNodeId` so the
//!    dispatch can resolve the macro payload.
//! 2. `dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload { .. })`
//!    yields the macro payload's semantic node (the resolved type that
//!    backs the macro instance).
//! 3. `dispatch.execute_read(SemanticQueryKey::ProjectPath { base, path: [], context: crate::semantic_query::ProjectionReductionContext::published(Shallow)})`
//!    enumerates the payload's surface members.
//! 4. For each surface member, the projector raises the member's value
//!    node back to `TypeExpr` and classifies its exactness via
//!    `meta_resolve::exactness::classify_node`.
//!
//! All `dispatch.execute_read` calls must dual-emit their
//! dep-signature into BOTH downstream channels via
//! `emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature)`:
//! (1) the legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` drained at
//! `compute_component_meta_state_inner` into `state.fact_versions`,
//! and (2) the `ACTIVE_TRACERS` stack captured by the outer
//! `with_fact_tracer` scope. Dual-emit is the migration substrate
//! that lets the `fact_dep_signature` producer source later flip
//! from `state.fact_versions` to `read_set.finalise()` without
//! losing coverage. The final-result cache validates warm hits
//! against `fact_dep_signature` on both branches. Cycle and error
//! branches publish a `MacroExpansionDiagnostics` envelope into
//! `diag_sink` (per §7.5 silent-miss prevention).
//!
//! The macro_index inside each projector identifies which macro this
//! projection corresponds to, for diagnostic correlation and for the
//! shape-merge logic in the parser-side analysis.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{
    ExpandedField, ExpansionDiagnostic, ExpansionExactness, ExpansionExecutionStatus,
    ExpansionStopReason,
};
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::TypeExpr;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember,
};
use crate::types::FileAnalysisSnapshot;

use super::dep_signature::emit_dispatch_dep_signature_facts;
use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;
use super::exactness::classify_node;

pub(crate) mod define_shapes;
pub(crate) mod emits;
pub(crate) mod exposed;
pub(crate) mod macro_payload_substrate;
pub(crate) mod model;
pub(crate) mod options;
pub(crate) mod props;
pub(crate) mod published_reducer;
pub(crate) mod slots;

// Substrate re-exports for the transit-shallow macro publication
// pipeline (see [`macro_payload_substrate`]). The `unused_imports`
// allowance parallels the `dead_code` allowance on each primitive at
// the definition site: not every macro kind currently consumes every
// substrate primitive, but the re-exports keep the boundary stable as
// new consumers wire in.
#[cfg(test)]
pub(crate) use macro_payload_substrate::EMIT_CARRIER_WALK_FUSE;
#[allow(unused_imports)]
pub(crate) use macro_payload_substrate::{
    resolve_emit_payload_to_conditional_root, resolve_macro_payload_diagnostic_probe,
    resolve_payload_surface_with_scope, MemberValueRole, PayloadSurfaceScope,
};

pub(crate) use define_shapes::project_define_macro_shapes;
pub(crate) use emits::project_emits;
pub(crate) use exposed::project_exposed;
pub(crate) use model::project_model;
pub(crate) use options::project_options;
pub(crate) use props::project_props;
pub(crate) use published_reducer::{
    reduce_published_field_types, type_expr_contains_reducible_operator,
};
pub(crate) use slots::project_slots;

/// Merge a projector's `Vec<ExpandedField>` output into the target
/// `Vec<ExpandedField>` on `evaluated_types`.
///
/// Per the project's component-meta shallow-by-default rule (see
/// `CLAUDE.md`), the projector pipeline is the **sole post-projection
/// authority** for finalising published field shapes. Parser-side
/// pre-population of `evaluated_types.props` (from
/// `verter_semantic::analysis::type_eval_build::expand_field_expr`
/// and friends) eagerly inlines alias bodies, which violates the
/// shallow-by-default contract for bare `Ref` references —
/// `defineProps<{ user: Foo }>` where `type Foo = string` lives in
/// the same file pre-populates `user`'s type as `Primitive(String)`,
/// not the bare `Ref { name: "Foo" }` that the rule mandates.
///
/// To honour the shallow-by-default invariant, the projector's
/// output always wins for fields the projector produced: when both
/// `target` and `projected` carry an entry with the same name, the
/// projected entry replaces the existing entry wholesale. Parser-
/// side fields the projector did NOT produce (entries that have no
/// name match in `projected`) are preserved as-is so prop annotations
/// the dispatch path didn't surface still appear in the published
/// analysis.
fn merge_projected_fields_by_name(
    target: &mut Vec<verter_semantic::analysis::type_expand::ExpandedField>,
    projected: Vec<verter_semantic::analysis::type_expand::ExpandedField>,
) {
    for field in projected {
        if let Some(existing) = target.iter_mut().find(|t| t.name == field.name) {
            if std::env::var("VERTER_PROJECTOR_MERGE_TRACE").is_ok() {
                eprintln!(
                    "[MERGE] name={} existing={:?} projected={:?}",
                    field.name, existing.r#type, field.r#type
                );
            }
            // Projector pipeline is the sole post-projection authority
            // — its output always replaces parser-side pre-population.
            *existing = field;
        } else {
            target.push(field);
        }
    }
}

/// Top-level driver that dispatches every type-based macro in the
/// snapshot through its per-kind projector and writes the resulting
/// fields into `evaluated_types`. The driver:
///
/// 1. For each `defineProps<T>`, calls [`project_props`] and extends
///    `evaluated_types.props` with the resulting fields.
/// 2. For each `defineEmits<T>`, calls [`project_emits`] and extends
///    `evaluated_types.emits`.
/// 3. For each `defineSlots<T>`, calls [`project_slots`]; the slot
///    fields are not directly published into `evaluated_types`
///    because the slot-shape level is consumed by
///    [`crate::meta_resolve::slot_binding_graph::resolve_slot_bindings_graph_native`]
///    via the same dispatch primitives. The diagnostic sink is the
///    only side-channel the projector contributes for slots.
/// 4. `defineModel`, `defineExpose`, `defineOptions` macros run their
///    projectors, but their downstream merge into the analysis lives
///    on the parser side; their projector results are only inspected
///    for diagnostic sink contributions.
///
/// Silent-miss prevention: every `Recursive` / `Error` branch the
/// projectors hit is appended to `diag_sink`, which the caller merges
/// into `analysis.macro_expansion_diagnostics`. A projector must never
/// silently return an empty surface on `QueryResult::Error` — that
/// would be indistinguishable from a successful empty result.
pub(crate) fn project_evaluated_types(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    file: &str,
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) {
    let owner = build_owner_decl_identity(query_engine.ctx, file);

    // Construct a `SurfaceProjection` per macro kind so each
    // projector entry receives a path-precise cursor.
    // `whole_surface(kind)` admits every published member name;
    // narrower cursors are threaded in when consumer demand is known.
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
                let fields = project_props(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
                merge_projected_fields_by_name(&mut evaluated_types.props, fields);
            }
            AnalyzedMacroKind::DefineEmits => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Emits);
                let fields = project_emits(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
                merge_projected_fields_by_name(&mut evaluated_types.emits, fields);
            }
            AnalyzedMacroKind::DefineSlots => {
                // Slot-shape projection is consumed by the
                // slot-binding-graph synthesis layer which
                // shares the same dispatch primitives; running the
                // projector here populates the diagnostic stream and
                // primes the dispatch family memo.
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Slots);
                let _ = project_slots(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
            AnalyzedMacroKind::DefineModel => {
                // The parser-side analysis owns the
                // `synthesize_model_prop_and_event` that publishes
                // the prop+event pair. Running the projector here
                // populates the diagnostic stream and primes the
                // dispatch family memo so the parser-side merge can
                // observe the resolved type without a second
                // resolution pass.
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Model);
                let _ = project_model(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
            AnalyzedMacroKind::DefineExpose => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Exposed);
                let _ = project_exposed(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
            AnalyzedMacroKind::DefineOptions => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Internal {
                    caller: "project_evaluated_types::define_options",
                });
                let _ = project_options(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
        }
    }
}

/// Identifier name used as the synthetic decl-name for a `<script setup>`
/// scope when the dispatch's `DeclIdentity` only consults the canonical
/// id + whole hash for cache keying. Mirrors the constant from
/// [`super::slot_binding_graph`] so projector code paths share the
/// same decl identity for `ResolveMacroPayload` cache keys.
pub(crate) const SFC_SCRIPT_SETUP_DECL_NAME: &str = "<sfc-script-setup>";

/// Build the owner [`DeclIdentity`] for an SFC's macro queries.
///
/// Mirrors `slot_binding_graph::build_owner_decl_identity` so the
/// dispatch keys produced by projectors collide with the existing
/// graph-native synthesis layer (path-independent caching per
/// `CLAUDE.md` Build Philosophy).
pub(crate) fn build_owner_decl_identity(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
) -> DeclIdentity {
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

/// Convert an `Arc<[PathSegment]>` empty path constant. Cached locally
/// so each projector call reuses a shared-ref-counted empty path.
#[inline]
pub(crate) fn empty_path() -> Arc<[PathSegment]> {
    Arc::from(Vec::<PathSegment>::new().into_boxed_slice())
}

// =============================================================================
// `peek_member_shape_known` — graph-native type-peek primitive.
//
// The peek is the projector pipeline's shallow-by-default enforcement
// substrate: the caller asks "do you already know the shape of this type
// expression at this scope / mode?" and the implementation answers
// WITHOUT triggering any reducer / resolver / route rebuild.
//
//  - `PeekedShape::Leaf` — the expression is a leaf primitive or literal;
//    publishing it is a clone.
//  - `PeekedShape::BareCarrier` — the expression is an unparameterised
//    `Ref` (a plain alias name). Per the shallow-by-default rule, the
//    projector publishes the Ref shallow; consumers re-resolve through
//    the registry on demand. No reduction needed.
//  - `PeekedShape::Cached` — the expression already has an entry in
//    `ShapeCacheDb` keyed on `(scope, expr, mode)`. The peek re-emits
//    the cached entry's `fact_dep_signature` into the active fact tracer
//    (the same `peek` protocol that the per-member slot of `ShapeCacheDb`
//    indexed by `ShapeSubject::SemanticNode` follows) so the cm-result
//    cache validation invariants are preserved.
//  - `None` — the cache is cold for this triple; the caller must decide
//    whether to reduce (operator-shape / generic instantiation cases) or
//    publish shallow (bare alias case already covered above).
//
// Strictly request-bound: the `debug_assert!` enforces that the caller's
// `ResolverContext` is request-bound. Bare-host invocation would force a
// workspace snapshot rebuild.
// =============================================================================

/// Result of peeking whether a type's shape is known cheaply.
///
/// `Some(_)` ⇒ the caller may publish or short-circuit WITHOUT
/// triggering reduction. `None` ⇒ the cache is cold; the caller must
/// reduce (or publish the Ref shallow per the shallow-by-default rule).
pub(crate) enum PeekedShape {
    /// The expression is a bare alias carrier; publish the Ref
    /// shallow. No reduction needed. `name` is exposed for
    /// `projectors_peek_tests` behavioural-discrimination assertions;
    /// production match arms use `_`.
    BareCarrier {
        #[allow(dead_code)]
        name: Arc<str>,
    },
    /// The expression is a leaf primitive / literal — publish as-is.
    Leaf(verter_type_expr::TypeExpr),
    /// The expression has already been reduced; return the cached
    /// `MaterializedTypeExpr` verbatim. The peek implementation
    /// re-emits the cached entry's `fact_dep_signature` into the
    /// active fact tracer + dispatch dep-signature accumulator via the
    /// `MaterializeMemoDb::peek` protocol (`bubble_fact_signature` in
    /// `component_meta_caches.rs:1346`).
    Cached(crate::project_semantic_dispatch::raise::MaterializedTypeExpr),
}

/// Peek without expansion.
///
/// Does NOT consult `RouteDb`, `OwnerImportSurfaceDb`, or rebuild any
/// workspace view — observes ONLY the request-bound store_view via
/// `ctx.store_view()` (through the cooperative `MaterializeMemoDb::peek`).
///
/// MUST be invoked from a request-bound context; the
/// `debug_assert!(query_engine.ctx.is_request_bound())` enforces this.
/// Reaching this from a bare-host context would force a workspace
/// snapshot rebuild.
pub(crate) fn peek_member_shape_known(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &TypeExpr,
    mode: ProjectionMode,
) -> Option<PeekedShape> {
    debug_assert!(
        query_engine.ctx.is_request_bound(),
        "peek_member_shape_known invoked from bare-host context — \
         would force a workspace snapshot rebuild"
    );

    match expr {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => Some(PeekedShape::Leaf(expr.clone())),
        TypeExpr::Ref {
            type_arguments,
            name,
        } if type_arguments.is_empty() => Some(PeekedShape::BareCarrier { name: name.clone() }),
        _ => {
            // Operator-shape (Pick/Omit/IndexedAccess/Conditional/Mapped)
            // or generic instantiation: consult `ShapeCacheDb` only.
            // Does NOT consult RouteDb / OwnerImportSurfaceDb (those
            // would rebuild HostStoreView and re-introduce the
            // per-query workspace-snapshot rebuild cost the peek path
            // exists to avoid).
            let ctx: &dyn ResolverContext = query_engine.ctx;
            // gap1: key the operator-shape slot by the EXACT reduction
            // context the whole-expression materialiser
            // (`materialize_component_meta_type_expr_until_stable_full`)
            // writes under, so this peek and that publish share one
            // cache identity. A bare `published(mode)` key would miss a
            // `StructuralTransit(Navigate)`-published entry (or, worse,
            // hit a published entry storing a transit-lowered value).
            let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
                Arc::<str>::from(scope_canonical_id),
                Arc::new(expr.clone()),
                super::materialize::type_expr_materialize_reduction_context(expr, mode),
            );
            ctx.project_type_store()
                .shape_cache_db()
                .peek(&key, ctx)
                .map(PeekedShape::Cached)
        }
    }
}

/// Read the [`SurfaceView`] members backing `node`, if `node` resolves
/// to a `SemanticNodeData::Object` shell. Empty for any other variant
/// — callers treat the empty surface as "no enumerable members".
///
/// Mirrors `slot_binding_graph::read_surface_members`.
pub(crate) fn read_surface_members(
    ctx: &dyn ResolverContext,
    surface_node: SemanticNodeId,
) -> Vec<SurfaceMember> {
    match crate::project_semantic_dispatch::node_data_for(ctx, surface_node).as_deref() {
        Some(SemanticNodeData::Object(view)) => view.members.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Build a [`MacroExpansionDiagnostics`] for a `QueryError` encountered
/// during projection. Mirrors `slot_binding_graph::macro_expansion_for_query_error`.
pub(crate) fn macro_expansion_for_query_error(
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

/// Build a [`MacroExpansionDiagnostics`] for a `Recursive` back-edge
/// encountered during projection. Mirrors
/// `slot_binding_graph::macro_expansion_for_cycle`. Cycles are not
/// fatal — they bound the published surface to the non-recursive arms.
pub(crate) fn macro_expansion_for_cycle(
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

/// Surface-provenance for a macro payload's own-body members (codex
/// BINDING design).
///
/// `defineProps<T>()` and `withDefaults(defineProps<T>(), …)` resolve a
/// PROP surface whose members the author literally wrote in the macro
/// type argument `T`; those members must carry
/// [`verter_session::semantic_query::SurfaceMember::declared_in_macro_type_arg`]
/// `= true`. The bit is consumed by
/// `verter_audit::PublishedSurfacePolicy::Refined` to distinguish
/// author-declared props from heritage-reached props on the PROPS axis
/// only. Every other macro kind (emits / slots / options / model /
/// expose) resolves a structural surface — the bit is always `false`
/// downstream for those kinds — so they lower with
/// [`SurfaceProvenanceContext::Structural`].
#[inline]
#[must_use]
pub(crate) fn macro_payload_surface_provenance(
    macro_kind: AnalyzedMacroKind,
) -> crate::semantic_query::SurfaceProvenanceContext {
    match macro_kind {
        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
            crate::semantic_query::SurfaceProvenanceContext::MacroTypeArgOwnBody
        }
        AnalyzedMacroKind::DefineEmits
        | AnalyzedMacroKind::DefineSlots
        | AnalyzedMacroKind::DefineModel
        | AnalyzedMacroKind::DefineExpose
        | AnalyzedMacroKind::DefineOptions => {
            crate::semantic_query::SurfaceProvenanceContext::Structural
        }
    }
}

/// Resolve a type-based macro's payload through `ResolveMacroPayload`.
///
/// Lowers the macro's `parsed_type_argument` to a [`SemanticNodeId`] in
/// `Navigate` mode, then dispatches `ResolveMacroPayload` and returns
/// the macro payload node on success.
///
/// On `Recursive` or `Error`, appends a diagnostic to `diag_sink` and
/// returns `None`. Dep-signature is accumulated unconditionally.
///
/// Silent-miss prevention (§7.5): when lowering itself fails (the
/// type expression cannot be lowered to a `SemanticNodeId` — e.g. an
/// unresolved import to a non-existent module), a diagnostic is
/// pushed before returning `None`.
pub(crate) fn resolve_macro_payload(
    dispatch: &ProjectSemanticDispatch<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    macro_kind: AnalyzedMacroKind,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    let parsed_arg = mac.parsed_type_argument.as_ref()?;
    // Surface-provenance for the macro type argument's own body (codex
    // BINDING design). Props / withDefaults carry the macro-T own-body
    // provenance so members written directly in `defineProps<T>()`'s `T`
    // surface with `declared_in_macro_type_arg = true`. Emits / slots /
    // options / model / expose are structural — their
    // `declared_in_macro_type_arg` is always `false` downstream (the bit
    // is a props-axis concern consumed by `PublishedSurfacePolicy::Refined`).
    let macro_provenance = macro_payload_surface_provenance(macro_kind);
    let type_args: Arc<[SemanticNodeId]> = match dispatch.lower_type_expr_in_scope_with_context(
        file,
        parsed_arg,
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            ProjectionMode::Navigate,
        )
        .with_provenance(macro_provenance),
    ) {
        Some(node) => Arc::from(vec![node].into_boxed_slice()),
        None => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-lowering-failed@{:?}", macro_kind),
            ));
            return None;
        }
    };

    let payload_read = dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload {
        owner: dispatch.type_slot_for(Arc::clone(&owner.canonical_id), Arc::clone(&owner.decl_name)),
        macro_index,
        macro_kind,
        type_args,
        context: dispatch.macro_payload_context_for(&owner.canonical_id, ProjectionMode::Navigate),
    });
    emit_dispatch_dep_signature_facts(dispatch.ctx, &payload_read.dep_signature);
    if !payload_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &payload_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            payload_read.cache_suppress,
        ));
    }

    let payload_node = match payload_read.value {
        QueryResult::Value(id) => id,
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                format!("cyclic-macro-payload@{:?}", macro_kind),
            ));
            return None;
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-error::{:?}", e),
            ));
            return None;
        }
    };

    // Silent-miss prevention (§7.5): when the dispatch returns an
    // opaque-as-value sentinel (i.e. the resolution stuck at an
    // unresolved declaration / cycle inside Navigate mode), publish
    // a diagnostic before bailing. Without this, callers see an
    // empty surface that's indistinguishable from a successful
    // empty payload.
    if let Some(SemanticNodeData::Opaque(err)) =
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node).as_deref()
    {
        diag_sink.push(macro_expansion_for_query_error(
            macro_index,
            expansion_kind,
            format!("macro-payload-opaque::{:?}", err),
        ));
        return None;
    }

    // Silent-miss compensation for the transit-shallow publication
    // contract. When the macro publication boundary lowers under
    // `Navigate` mode + `Published(Shallow)` terminal (slot path),
    // the dispatch chain no longer eagerly resolves `DeclRef`
    // carriers for unresolved imports — the cached payload becomes
    // an EMPTY `Object` surface instead of an `Opaque` sentinel, so
    // the check above silently passes. Detect the empty-surface
    // payload and probe the macro's `parsed_type_argument` via
    // `Published(Expanded)` lowering: an unresolved declaration
    // surfaces as `Opaque(DeclPlaceholder)` under eager resolution
    // and the diagnostic fires here. Legitimate empty macros
    // (`defineProps<{}>()`) pass through because the eager lowering
    // returns a non-`Opaque` empty Object.
    let payload_is_empty_surface = matches!(
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node).as_deref(),
        Some(SemanticNodeData::Object(view))
            if view.members.is_empty()
                && view.call_signatures.is_empty()
                && view.construct_signatures.is_empty()
                && view.index_signatures.is_empty()
    );
    if payload_is_empty_surface {
        if let Some(parsed_arg) = mac.parsed_type_argument.as_ref() {
            if let Some(probe_node) = dispatch.lower_type_expr_in_scope_with_mode(
                file,
                parsed_arg,
                ProjectionMode::Expanded,
            ) {
                if let Some(SemanticNodeData::Opaque(err)) =
                    crate::project_semantic_dispatch::node_data_for(dispatch.ctx, probe_node)
                        .as_deref()
                {
                    diag_sink.push(macro_expansion_for_query_error(
                        macro_index,
                        expansion_kind,
                        format!("macro-payload-decl-unresolved::{:?}", err),
                    ));
                    return None;
                }
            }
        }
    }

    Some(payload_node)
}

pub(crate) fn resolve_payload_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    provenance: crate::semantic_query::SurfaceProvenanceContext,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    // The empty-path `ProjectPath` carries the macro's surface
    // provenance (codex BINDING design): for a props payload that
    // resolved to a `DeclRef` carrier (`defineProps<FooProps>()`), the
    // walker's `DeclPlaceholder` expansion preserves the
    // `MacroTypeArgOwnBody` provenance onto its `Instantiate`, so
    // `FooProps`'s OWN-body members surface with
    // `declared_in_macro_type_arg = true`. Structural-provenance kinds
    // (emits / slots / …) pass `Structural` and observe `false`.
    // Vue macro object-surface publication. The macro
    // surface enumerates the UNION of object-arm members
    // (`defineProps<FixedProps | BubbleProps>()` declares every arm's
    // props), NOT the TS property-access common-member intersection that
    // an ordinary `Published(Shallow)` ProjectPath would synthesise. The
    // `MacroObjectSurface` demand selects the union-arm rule at the
    // empty-path Shallow terminal surface and is cache-keyed in a distinct
    // `ModeSlot` so the macro surface never collides with an ordinary
    // `Published(Shallow)` read of the same payload node. `provenance`
    // (macro-T own-body for props; structural for slots / emits) rides on
    // the context unchanged.
    let surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: payload_node,
        path: empty_path(),
        context: crate::semantic_query::ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            provenance,
        ),
    });
    emit_dispatch_dep_signature_facts(dispatch.ctx, &surface_read.dep_signature);
    if !surface_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &surface_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            surface_read.cache_suppress,
        ));
    }
    match surface_read.value {
        QueryResult::Value(id) => Some(id),
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                "cyclic-macro-payload-surface".to_string(),
            ));
            None
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-surface-error::{:?}", e),
            ));
            None
        }
    }
}

// Macro-payload boundary substrate primitives — diagnostic probe,
// scope tag, branch-merged scope-gated resolver, MemberValueRole —
// live in the sibling [`macro_payload_substrate`] module to keep this file
// under the `no_oversize_files` architecture-guard cap. The
// primitives are re-exported at the module top so call sites
// continue to import them as `crate::meta_resolve::projectors::*`.

/// Peek-before-raise per-member helper.
///
/// Wraps the cold compute path for one `(scope, member_value, mode)`
/// triple around the host-owned per-member slot of
/// [`crate::component_meta_caches::ShapeCacheDb`] (indexed by
/// [`crate::component_meta_caches::ShapeSubject::SemanticNode`] via
/// `ShapeCacheKey::semantic_node_whole`). The contract:
///
///  1. **Peek first.** Warm hits return the cached
///     [`crate::project_semantic_dispatch::raise::MaterializedTypeExpr`]
///     WITHOUT paying any raise or gate cost — the goal of the cache
///     is that the per-member hot path returns in `peek` time.
///  2. **Cold path raises once.** A cold miss raises
///     `member_value` to a `TypeExpr` shell via
///     [`crate::project_semantic_dispatch::ProjectSemanticDispatch::raise_node_to_type_expr`],
///     then runs the same shallow gates `reduce_field_type_expr` runs
///     today (`type_expr_has_package_backed_object_like_root` +
///     `lowered_root_reaches_transitive_cycle` +
///     `type_expr_contains_reducible_operator`). The gates stay
///     TypeExpr-keyed in this block per the codex caveat — migrating
///     them to graph-native predicates widens blast radius into the
///     cycle module and is punted to a follow-up.
///  3. **Gate-rejected outcomes do NOT admit.** The raised TypeExpr
///     is returned verbatim wrapped in a `MaterializedTypeExpr`
///     envelope. Admitting a gate-rejected entry would store the
///     raised input verbatim — the cache would grow for no compute
///     win, since the gates are cheap to re-run.
///  4. **Cold compute is single-shot.** When a reduction is required,
///     `reduce_member_value_graph_native` runs ONCE
///     (per the C2 single-compute pattern). The cache's
///     `get_or_compute` closure captures the pre-computed
///     `MaterializedTypeExpr` by move; if the fact signature cannot
///     be built (no tear-free scope observation or a
///     `RouteGeneration`-tagged dep), admission is refused but the
///     pre-computed value is still returned to the caller — no second
///     reducer call.
fn member_shape_peek_or_compute(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    member_value: SemanticNodeId,
    mode: ProjectionMode,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;

    let ctx: &dyn ResolverContext = query_engine.ctx;
    // gap1: key the per-member SemanticNode slot by the EXACT reduction
    // context the cold path reduces under
    // (`type_expr_materializer_context(mode)` — `StructuralTransit` for
    // `Navigate`, `Published` otherwise). A bare `published(mode)` key
    // collided a transit-lowered carrier publication with a published
    // consumer over the same `(scope, node)`.
    let member_reduction_context = super::materialize::type_expr_materializer_context(mode);
    let key = crate::component_meta_caches::ShapeCacheKey::semantic_node_whole_with_context(
        Arc::<str>::from(scope_canonical_id),
        member_value,
        member_reduction_context,
    );

    // (1) Peek FIRST — warm path pays zero raise/gate cost. The cached
    // entry's dep_signature must be re-emitted into the active fact
    // tracer + dispatch dep-signature accumulator so the request's
    // dep set sees the same facts the cold compute emitted.
    let cache = ctx.project_type_store().shape_cache_db();
    if let Some(cached) = cache.peek(&key, ctx) {
        emit_dispatch_dep_signature_facts(ctx, &cached.dep_signature);
        return cached;
    }

    // (2) Cold path: raise once.
    let raised = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        dispatch
            .raise_node_to_type_expr(member_value)
            .unwrap_or(TypeExpr::Unknown { raw: String::new() })
    };

    // (3) Shallow gates on the raised TypeExpr — same predicates
    // `reduce_field_type_expr` runs today. Gates run BEFORE the
    // operator-shape peek consultation: `MaterializeMemoDb` is shared
    // across the typed-IR materialiser callers (model resolution /
    // registry candidate materialisation) which do not apply these
    // projector shallow gates. Honouring the gates first ensures a
    // warm cache hit on `External['x']` (or any package-backed root)
    // publishes as the shallow carrier the shallow-by-default rule
    // requires, rather than as the reduced body the cache happens to
    // hold.
    //
    // F1: capture the gate-observed dep fences so the admit paths
    // below thread them into the cache entry's `fact_dep_signature`.
    // Without this, gate-shortcut entries would self-root only on the
    // scope file's whole_hash — stale on cross-file edits to the
    // declaring helper / package-backing file / cycle BFS roots.
    //
    // H2: the gate's fence is now `Option<DepSignature>`. `None`
    // means "refuse shared admission" — the gate observed an
    // unavailable `authoritative_current_content_hash` for a
    // contributing canonical and cannot root the verdict on the
    // file state it was actually decided against. Callers MUST
    // skip the admit and return the value verbatim.
    let (route_is_package_backed, package_backed_fence_opt) =
        super::materialize::type_expr_has_package_backed_object_like_root_with_fence(
            &raised,
            scope_canonical_id,
            query_engine,
        );
    if route_is_package_backed {
        // Gate short-circuit: the published shape stays as the
        // raised carrier (package-backed roots are shallow). Admit
        // so sibling members of the same package-backed parent
        // (e.g. siblings of `Foo['a']` when `Foo` is from a package)
        // reuse this verdict at peek time rather than re-running the
        // package-backed predicate.
        // F1: thread the gate's cross-file fence into the admit so
        // edits to the package-backing declaration file invalidate.
        // H2: refuse admission if the gate refused.
        let Some(package_backed_fence) = package_backed_fence_opt.clone() else {
            // Refused fence — return the verdict verbatim without
            // admitting to the shared cache. A subsequent call
            // recomputes the verdict against the then-current
            // file state.
            return MaterializedTypeExpr {
                node_id: Some(member_value),
                type_expr: raised,
                dep_signature: Arc::from(Vec::new()),
                cache_suppress: false,
            };
        };
        let value = MaterializedTypeExpr {
            node_id: Some(member_value),
            type_expr: raised,
            dep_signature: package_backed_fence,
            cache_suppress: false,
        };
        return admit_member_shape_if_possible(ctx, &key, value);
    }
    // Non-package-backed path: unwrap the gate's fence Option. The
    // gate refuses (`None`) only when a contributing canonical's
    // authoritative hash is unavailable; for non-package-backed
    // verdicts the gate returns `Some(empty_fence)` because there
    // ARE no contributing cross-file canonicals to root.
    let Some(package_backed_fence) = package_backed_fence_opt else {
        // The gate refused for some reason that surfaced even on
        // the non-package-backed return path (a pre-emption between
        // the workspace check and the fence push). Refuse admission
        // for any downstream cache path that depends on this gate.
        return MaterializedTypeExpr {
            node_id: Some(member_value),
            type_expr: raised,
            dep_signature: Arc::from(Vec::new()),
            cache_suppress: false,
        };
    };
    // F1: cycle-gate fence — computed lazily because the cycle gate
    // only fires on generic instantiations. For non-generic raised
    // shapes the cycle fence stays empty.
    let is_generic_instantiation =
        matches!(&raised, TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty());
    let cycle_fence: crate::semantic_query::DepSignature = if is_generic_instantiation {
        let (reaches_cycle, fence) = super::lowered_root_reaches_transitive_cycle_with_fence(
            query_engine,
            scope_canonical_id,
            &raised,
        );
        if reaches_cycle {
            // Recursive parameterised helper: the published shape stays
            // as the raised carrier (the cycle prevents finite reduction).
            // Admit so subsequent peeks skip the cycle BFS.
            // F1: combine both gate fences (package-backed + cycle BFS)
            // so the admit's `fact_dep_signature` invalidates on edits
            // to any visited declaration file.
            let combined_fence =
                combine_dep_signatures(&package_backed_fence, &fence, scope_canonical_id);
            let value = MaterializedTypeExpr {
                node_id: Some(member_value),
                type_expr: raised,
                dep_signature: combined_fence,
                cache_suppress: false,
            };
            return admit_member_shape_if_possible(ctx, &key, value);
        }
        fence
    } else {
        Arc::from(Vec::new())
    };
    // The carrier-stop decision lives on the dispatch-layer
    // reduction-demand context, NOT on a projector-side name
    // predicate. The demand axis lives on every `Instantiate` /
    // `KeyOf` / `MappedType` key, so a userland `MyPick<T,K>` and
    // the builtin `Pick<T,K>` follow the same path. Generic
    // instantiations enter the reducer; the dispatch carrier-stops
    // downstream operators when the context does not admit reduction.
    let needs_reduction =
        type_expr_contains_reducible_operator(&raised) || is_generic_instantiation;
    // F1: combined gate fence threaded through every remaining admit
    // path so the cache entries do not self-root on the scope file
    // only.
    let gate_fence =
        combine_dep_signatures(&package_backed_fence, &cycle_fence, scope_canonical_id);
    if !needs_reduction {
        // Universal-caching invariant: a shape that resolves to a
        // non-reducible TypeExpr (primitive / literal / bare alias /
        // closed object / function / union / intersection without
        // operator nodes) is a STABLE shape — admit it so sibling
        // members hitting the same `SurfaceMember.value` (or the
        // same `(scope, node, mode)` triple from a downstream call)
        // short-circuit at peek time.
        let value = MaterializedTypeExpr {
            node_id: Some(member_value),
            type_expr: raised,
            dep_signature: gate_fence,
            cache_suppress: false,
        };
        return admit_member_shape_if_possible(ctx, &key, value);
    }

    // Gates have cleared: consult the operator-shape peek. A warm
    // `ShapeCacheDb` hit on `(scope, raised, mode)` now SAFELY
    // short-circuits the cold reducer dispatch — the entry's
    // semantics are compatible with the projector's published shape
    // because the package-backed and cycle paths already returned
    // above.
    //
    // Universal-caching invariant: when the peek returns `Leaf` /
    // `BareCarrier` we still admit a SemanticNode-subject entry to
    // the cache via `admit_computed` so subsequent member peeks
    // short-circuit at peek time.
    if let Some(peeked) = peek_member_shape_known(query_engine, scope_canonical_id, &raised, mode) {
        match peeked {
            PeekedShape::Leaf(leaf) => {
                let value = MaterializedTypeExpr {
                    node_id: Some(member_value),
                    type_expr: leaf,
                    dep_signature: gate_fence,
                    cache_suppress: false,
                };
                return admit_member_shape_if_possible(ctx, &key, value);
            }
            PeekedShape::BareCarrier { .. } => {
                let value = MaterializedTypeExpr {
                    node_id: Some(member_value),
                    type_expr: raised,
                    dep_signature: gate_fence,
                    cache_suppress: false,
                };
                return admit_member_shape_if_possible(ctx, &key, value);
            }
            PeekedShape::Cached(materialized) => {
                // Warm operator-shape hit AFTER gate validation. The
                // cached entry already observed `dep_signature`; the
                // peek bubbled it. Return verbatim.
                return materialized;
            }
        }
    }

    // (4) Cold compute via the graph-native reducer. Single-shot —
    // pre-computed ONCE outside the cache call (C2 single-compute
    // pattern). The cache's `get_or_compute` closure either captures
    // and moves the pre-computed `materialized` into the cache entry,
    // or returns `None` (signature-refusal) — in either case the
    // pre-computed value is the correct answer; no second reducer
    // call.
    //
    // The reducer uses the same demand context as the TypeExpr-start
    // materializer: `Expanded` remains whole-surface publication, while
    // per-prop `Navigate` is a structural-transit carrier publication
    // that does not enumerate mapped/keyof interiors. The cache `key`
    // continues to use the caller's `mode` so carrier publication does
    // not collide with an `Expanded` consumer slot.
    let reduction_context = super::materialize::type_expr_materializer_context(mode);
    let materialized = super::materialize::reduce_member_value_graph_native_with_context(
        ctx,
        scope_canonical_id,
        member_value,
        reduction_context,
    );
    let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
    // F1: merge the gate fence into the materialised entry's dep
    // signature so the cold-path admit's `fact_dep_signature` also
    // captures the gates' cross-file observations. Without this, the
    // cold-path admit would self-root only on `scope` + the reducer's
    // observed deps, missing gate-only deps (e.g., package-backed
    // declaration scope) that should invalidate.
    let materialized_with_gate_fence =
        merge_gate_fence_into_materialized(materialized.clone(), &gate_fence, scope_canonical_id);
    let materialized_for_closure = materialized_with_gate_fence.clone();
    let admitted = cache.get_or_compute(&key, ctx, move || {
        let scope_obs = observed_scope?;
        let parse_fact = scope_obs.syntactic_export_set.clone()?;
        match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
            &scope_obs,
            parse_fact,
            &materialized_for_closure.dep_signature,
        ) {
            crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                Some((materialized_for_closure, sig.facts))
            }
            crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
        }
    });
    admitted.unwrap_or(materialized_with_gate_fence)
}

/// F1 helper: combine two `DepSignature` slices, dropping duplicate
/// `(canonical, DepVersion)` entries and the scope's self-entry (the
/// scope is self-rooted by `engine_fact_signature_for_materialize_memo`).
///
/// Order is preserved (first occurrence wins) so the resulting
/// signature is deterministic given deterministic inputs.
fn combine_dep_signatures(
    a: &crate::semantic_query::DepSignature,
    b: &crate::semantic_query::DepSignature,
    scope_canonical_id: &str,
) -> crate::semantic_query::DepSignature {
    let mut out: Vec<(Arc<str>, crate::semantic_query::DepVersion)> =
        Vec::with_capacity(a.len() + b.len());
    let mut seen: rustc_hash::FxHashSet<(Arc<str>, crate::semantic_query::DepVersion)> =
        rustc_hash::FxHashSet::default();
    for entry in a.iter().chain(b.iter()) {
        if entry.0.as_ref() == scope_canonical_id || entry.0.as_ref().is_empty() {
            continue;
        }
        let pair = (Arc::clone(&entry.0), entry.1.clone());
        if seen.insert(pair.clone()) {
            out.push(pair);
        }
    }
    Arc::from(out.into_boxed_slice())
}

/// F1 helper: append the gate fence's dep entries to the materialised
/// `MaterializedTypeExpr.dep_signature`, deduplicating against the
/// already-observed entries. Used on the cold-compute admit path so
/// the entry's fact signature captures BOTH the reducer's observed
/// deps AND the gate-observed deps.
fn merge_gate_fence_into_materialized(
    mut materialized: crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
    gate_fence: &crate::semantic_query::DepSignature,
    scope_canonical_id: &str,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    if gate_fence.is_empty() {
        return materialized;
    }
    let combined =
        combine_dep_signatures(&materialized.dep_signature, gate_fence, scope_canonical_id);
    materialized.dep_signature = combined;
    materialized
}

/// Admit a freshly-computed SemanticNode-subject shape into the
/// universal [`crate::component_meta_caches::ShapeCacheDb`] when the
/// scope has a tear-free `observe_materialize_scope` observation.
///
/// Universal-caching invariant: every successful `(node, scope, mode)`
/// shape compute admits so sibling members and future peeks return
/// the cached value rather than re-paying the raise + gate cost.
///
/// Falls through to returning the value verbatim when the
/// observation is unavailable (session tombstone / evicted scope /
/// no recoverable `IndexedReady`) — without a view-correct scope
/// identity to self-root, admitting would mis-root the entry. This
/// is the documented degradation path; the caller still receives
/// the same value the cold compute produced.
/// Admit a TypeExpr-subject shape into the universal
/// [`crate::component_meta_caches::ShapeCacheDb`] when the scope has
/// a tear-free observation. Used by the `reduce_field_type_expr`
/// peek primitive's `Leaf` / `BareCarrier` arms to enforce the
/// universal-caching invariant: every successful shape compute
/// admits, regardless of how cheap the compute was.
///
/// F1 — callers MUST pass the `dep_signature` capturing any cross-file
/// dependencies observed during the path that produced
/// `materialized_type_expr`. Passing an empty signature means the
/// admitted entry self-roots ONLY on the scope file's whole_hash, so
/// edits to other files the gate/compute touched would not invalidate
/// the cached verdict. For Leaf/BareCarrier admits in
/// `reduce_field_type_expr`, the caller threads the gate fence (cycle
/// + package-backed) collected upstream.
fn admit_type_expr_shape_if_possible(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
    mode: ProjectionMode,
    materialized_type_expr: TypeExpr,
    dep_signature: crate::semantic_query::DepSignature,
) {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    let value = MaterializedTypeExpr {
        node_id: None,
        type_expr: materialized_type_expr,
        dep_signature,
        cache_suppress: false,
    };
    // gap1: admit into the SAME slot identity the whole-expression
    // materialiser + `peek_member_shape_known` use — keyed by the exact
    // reduction context, not a bare `published(mode)`.
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
        Arc::<str>::from(scope_canonical_id),
        Arc::new(expr.clone()),
        super::materialize::type_expr_materialize_reduction_context(expr, mode),
    );
    let _ = admit_member_shape_if_possible(ctx, &key, value);
}

/// Universal-caching admission for the projector pipeline. Computes
/// the `fact_dep_signature` from the value's `dep_signature` + scope
/// observation, then delegates to
/// [`crate::component_meta_caches::ShapeCacheDb::admit_computed`] —
/// the single centralised admission point that handles the
/// `get_or_compute` invocation and the verbatim fallback when
/// admission is refused.
///
/// Returns the input `value` verbatim when:
/// - the scope observation cannot be obtained (no scope view);
/// - the scope has no `syntactic_export_set` parse fact;
/// - the engine fact-signature builder refuses (overflow / missing
///   provenance).
///
/// In all refusal cases the caller receives the same
/// `MaterializedTypeExpr` it computed — admission is best-effort.
fn admit_member_shape_if_possible(
    ctx: &dyn ResolverContext,
    key: &crate::component_meta_caches::ShapeCacheKey,
    value: crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    let scope = key.subject.scope_canonical().clone();
    let Some(observed_scope) = ctx.observe_materialize_scope(scope.as_ref()) else {
        return value;
    };
    let Some(parse_fact) = observed_scope.syntactic_export_set.clone() else {
        return value;
    };
    let fact_sig = match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
        &observed_scope,
        parse_fact,
        &value.dep_signature,
    ) {
        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => sig.facts,
        crate::cache_runtime::SignatureAdmission::NonCacheable(_) => return value,
    };
    ctx.project_type_store()
        .shape_cache_db()
        .admit_computed(key, ctx, value, fact_sig)
}

/// Build an [`ExpandedField`] for a single surface member.
///
/// Raises the member's value node back to a [`TypeExpr`] (falling back
/// to `TypeExpr::Unknown` if raise fails), classifies its exactness
/// through the shared [`classify_node`] predicate, then runs the
/// bounded fixed-point reducer on the raised expression so nested
/// `IndexedAccess` chains collapse to concrete leaves.
///
/// `raw_type` is taken from the parser's `analyzed_prop.type_annotation`
/// when available. The caller passes `None` when no analyzed prop
/// matches the surface member's name.
///
/// The member's value is also resolved through one additional
/// `ProjectPath { mode: Shallow }` so that `DeclRef` carriers
/// (the terminal Navigate-mode form for unparameterised type
/// aliases) collapse to their underlying primitive / object /
/// function shape. Without this hop, `defineProps<{ msg: MyStr }>`
/// where `type MyStr = string` would publish `msg` as
/// `ExactSymbolic`.
///
/// The bounded fixed-point reducer
/// ([`materialize_component_meta_type_expr_until_stable`]) makes
/// the projector self-sufficient for nested `IndexedAccess` shapes
/// (e.g. `Pick<Foo, 'a'>['a']['nested']`). Generic substitutions
/// travel through the dispatch `lower → raise_and_reduce` pipeline
/// inside the reducer; cache keys include the relevant scope / expr
/// / mode tuple, dep_signature is accumulated into the per-request
/// thread-local accumulator, and any dispatch fence
/// `MacroExpansionDiagnostics` flow through the same accumulator
/// the projector's other dispatches use.
pub(crate) fn surface_member_to_expanded_field(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    member: &SurfaceMember,
    raw_type: Option<String>,
    shallow_type_expr: Option<TypeExpr>,
    shallow_type_expr_scope: Option<verter_type_expr::TypeExprScope>,
    member_cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> ExpandedField {
    let ctx: &dyn ResolverContext = query_engine.ctx;
    // The publication mode comes from the `member_cursor`.
    // `Navigate` (carrier) mode means the member's type body is
    // published as a carrier `Ref`, not breadth-expanded.
    let publish_mode = member_cursor.terminal_publication_mode();
    let carrier_mode = matches!(publish_mode, ProjectionMode::Navigate);
    // Exactness classification is independent of the member's reduced
    // TypeExpr; it walks the member's resolved-value graph. Keep it
    // isolated in its own dispatch scope so the peek-before-raise
    // contract for the type reduction is not coupled to a TypeExpr
    // raise that exactness does not need. In carrier mode the
    // classification does NOT expand a generic instantiation to its
    // object surface — that would re-open the breadth leak.
    let exactness = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let resolved_value =
            resolve_member_value_for_classification(&dispatch, member.value, carrier_mode);
        classify_node(&dispatch, resolved_value)
    };
    // Peek the per-member graph-native materialiser cache BEFORE any
    // `raise_node_to_type_expr(member.value)` call. Warm hits return
    // the cached `MaterializedTypeExpr` without paying the raise cost
    // or the shallow-gate cost; cold misses raise once, run the
    // gates, then dispatch the graph-native reducer + admit.
    //
    // `publish_mode` (from the `member_cursor`, computed above)
    // drives the per-member materialise. `Navigate` keeps a generic
    // instantiation `Tool<INPUT, OUTPUT>` as a `Ref` carrier instead
    // of breadth-enumerating `Tool`'s own members into the published
    // surface — Rule-5 shallow-by-default depth gate.
    let materialized =
        member_shape_peek_or_compute(query_engine, scope_canonical_id, member.value, publish_mode);
    let r#type = materialized.type_expr;
    debug_assert_eq!(
        shallow_type_expr.is_some(),
        shallow_type_expr_scope.is_some(),
        "ExpandedField (surface member `{}`) shallow_type_expr/shallow_type_expr_scope pairing violated",
        member.name.as_ref(),
    );
    // Gap3 provenance downgrade through transparent carriers: a member reached
    // ONLY via REAL heritage (`extends PlainProps` / `extends Vendor`) is NOT an
    // own-body member of the macro type argument, so it MUST carry
    // `declared_in_macro_type_arg = false` even though the macro-T own-body
    // synthesis can over-stamp the raw bit `true` on a heritage-reached member.
    // The `merge_role` is INDEPENDENTLY baked per arm (`Heritage` for
    // `extends`-reached, `OwnBody` for the declaration's own body), so it is the
    // authoritative discriminator. This is the SAME downgrade
    // `props_from_typeinfo_surface` applies on the DTO path — applying it here
    // keeps the flat `evaluated_types.props` field (which `define_props_shape`
    // reads first) in agreement, so an own-body member keeps `true` and a
    // heritage-reached member downgrades to `false`. NOT
    // `source_field.unwrap_or(false)`: that would also strip own-body members
    // (the cross-file-simple discriminating positive test rejects that accident).
    let declared_in_macro_type_arg = member.declared_in_macro_type_arg
        && member.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
    ExpandedField {
        name: member.name.as_ref().to_string(),
        r#type,
        raw_type,
        optional: member.optional,
        exactness,
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
        shallow_type_expr,
        shallow_type_expr_scope,
        declared_in_macro_type_arg,
    }
}

/// Drive the shared field-type reduction used by every projector and
/// by [`reduce_published_field_types`] on slot bindings, model bindings,
/// and any leftover parser-side fields.
///
/// # Shallow-by-default invariant
///
/// Per the project's component-meta shallow-by-default rule (see
/// `CLAUDE.md`), types and properties are ALWAYS published shallow at
/// the projector surface UNLESS the consumer explicitly walks the path.
/// Concretely:
///
/// - Plain alias references (`type Foo = ...`, including same-file and
///   imported aliases) MUST stay as `TypeExpr::Ref { name: "Foo" }`.
///   The projector does NOT eagerly inline the alias body. Consumers
///   re-resolve the alias through the registry on demand.
/// - `Pick<Foo, "bar">` materialises ONLY the `bar` member; other
///   `Foo` properties stay shallow. This is path-precise.
/// - `Foo['a']['b']` materialises only the `a` and `b` hops.
/// - Imported alias names (workspace-owned OR package-backed) stay
///   shallow regardless of where they live.
///
/// This function therefore reduces ONLY when the expression carries an
/// operator-shape node (`IndexedAccess` / `KeyOf` / `TypeOf` /
/// `Conditional` / `Mapped` / `Infer` / `Rest` / `TemplateLiteral`)
/// AND the route's root is not a package-backed object surface. Bare
/// `TypeExpr::Ref { .. }` inputs — whether their declaration body
/// resolves to a primitive, a utility wrapper, or any other shape —
/// are returned verbatim. The bounded fixed-point reducer
/// [`materialize_component_meta_type_expr_until_stable`] is the
/// authoritative reduction primitive for the operator case; generic
/// substitutions, dep-signature accumulation, and fence-validated
/// publication all flow through it.
pub(crate) fn reduce_field_type_expr(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: TypeExpr,
) -> TypeExpr {
    // Backward-compatible entry point — defaults to `Expanded`
    // publication mode (no carrier narrowing). Callers that publish
    // a macro surface shallow-by-default route through
    // `reduce_field_type_expr_with_mode` with `Navigate`.
    reduce_field_type_expr_with_mode(
        query_engine,
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )
}

/// Carrier-aware variant of [`reduce_field_type_expr`].
///
/// When `publish_mode` is `Navigate` (the shallow-by-default macro
/// publication boundary), arbitrary userland generic instantiations
/// (`Tool<INPUT, OUTPUT>`) are returned AS CARRIERS — the second-
/// pass reducer does NOT re-expand what the projector pipeline
/// deliberately kept shallow. Explicit narrowing operators
/// (`IndexedAccess`, finite `Pick`/`Omit`/other built-in utilities)
/// STILL reduce path-precisely: those are explicit consumer demand
/// inside the type expression.
pub(crate) fn reduce_field_type_expr_with_mode(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: TypeExpr,
    publish_mode: ProjectionMode,
) -> TypeExpr {
    // `publish_mode` drives both the peek key and the cold-path
    // materialiser dispatch so a per-prop `Navigate` publication
    // does not collide with an `Expanded` consumer slot. The
    // dispatch's reduction-demand context (`Published` vs
    // `StructuralTransit`) remains the sole carrier-stop gate at the
    // operator level; `publish_mode` carries the caller's mode
    // through the cache slot and into the iterative reducer.
    // Peek-before-reduce. Short-circuit when the expression's shape
    // is already known cheaply:
    //
    //   * `Leaf(_)`: a primitive / literal — publish the leaf
    //     verbatim.
    //   * `BareCarrier { name }`: a plain `Ref { type_arguments: [] }`.
    //     Per the shallow-by-default rule the projector publishes the
    //     Ref shallow; consumers re-resolve through the registry on
    //     demand.
    //   * `Cached(materialized)`: `MaterializeMemoDb` carries a warm
    //     `(scope, expr, mode)` entry. The cached value is published
    //     AFTER the package-backed gate (see below) because
    //     `MaterializeMemoDb` is shared with non-projector callers
    //     (model / registry materialiser paths) that do not apply
    //     the projector's shallow gate.
    //   * `None`: cold path; fall through to the package-backed gate
    //     + cycle guard + reducer.
    //
    // For `Leaf` and `BareCarrier` we short-circuit immediately — they
    // encode the shallow-by-default invariant structurally (primitive
    // / literal / bare alias name) and are independent of the route's
    // package-backing.
    //
    // Universal-caching invariant: when the peek returns `Leaf` /
    // `BareCarrier`, ADMIT a TypeExpr-subject entry into
    // `ShapeCacheDb` so subsequent TypeExpr-start callers (e.g.
    // `materialize_component_meta_type_expr_until_stable_full` peek
    // path) hit at peek time. Peek-time admission is cheap (no raise
    // / no reducer dispatch); the cache write is the universal-
    // caching contract.
    if let Some(peeked) =
        peek_member_shape_known(query_engine, scope_canonical_id, &expr, publish_mode)
    {
        match peeked {
            PeekedShape::Leaf(leaf) => {
                // F1: Leaf admission is a STRUCTURAL classification of
                // `expr` (Primitive / Literal). It does not depend on
                // any other file — `peek_member_shape_known` does not
                // touch cross-file state for these arms. Empty
                // dep_signature is correct here; the cache entry
                // self-roots on the scope file only.
                admit_type_expr_shape_if_possible(
                    query_engine.ctx,
                    scope_canonical_id,
                    &expr,
                    publish_mode,
                    leaf.clone(),
                    Arc::from(Vec::new()),
                );
                return leaf;
            }
            PeekedShape::BareCarrier { .. } => {
                // H1: skip the universal-cache admit for bare alias
                // `Ref { type_arguments: [] }` carriers.
                //
                // The TypeExpr cache slot
                // `ShapeCacheKey::type_expr_whole(scope, expr,
                // Expanded)` is ALSO the slot
                // `materialize_component_meta_type_expr_until_stable_full`
                // probes BEFORE dispatching its expansion pipeline.
                // Admitting the projector's shallow `Ref` here would
                // poison that slot: a subsequent materializer call
                // asking for the bare alias's EXPANDED body would
                // short-circuit on the cached `Ref` and skip
                // alias-body expansion.
                //
                // Bare alias re-resolution is cheap (a `Ref` lookup
                // is structural — `peek_member_shape_known` classifies
                // it in one match arm without any cross-file or
                // reducer work). The cost of NOT admitting a shallow
                // alias here is small; the cost of admit-collision
                // is correctness-breaking. The `Leaf` admit above
                // stays — `Primitive`/`Literal` are terminal shapes
                // that the materializer cannot expand further, so
                // the cache slot's shallow/expanded forms agree.
                return expr;
            }
            PeekedShape::Cached(_) => {
                // Fall through to the package-backed gate; we re-peek
                // below once the gate has cleared.
            }
        }
    }

    let route_is_package_backed = super::materialize::type_expr_has_package_backed_object_like_root(
        &expr,
        scope_canonical_id,
        query_engine,
    );
    if route_is_package_backed {
        return expr;
    }

    // Shallow-by-default invariant: a *plain* alias reference (a
    // `Ref` with empty `type_arguments`) is NEVER reduced here. The
    // projector publishes alias names as carriers; consumers re-
    // resolve through the registry on demand. Reduction fires only
    // when the consumer explicitly walked a path:
    //
    // - operator-shape nodes (`IndexedAccess`/`KeyOf`/`TypeOf`/
    //   `Conditional`/`Mapped`/`Infer`/`Rest`/`TemplateLiteral`),
    // - generic instantiations (`Ref` with non-empty `type_arguments`)
    //   such as `Pick<Foo,'a'>` / `Omit<Foo,'a'>` / `Partial<Foo>` /
    //   `Required<Foo>` / userland generic type aliases.
    //
    // Recursive parameterised helpers (`type GetItemKeys<T> = ...
    // GetItemKeys<...> ...`) carry non-empty `type_arguments` but
    // resolve through a self-referential cycle. The shared
    // transitive-cycle guard short-circuits reduction so the helper
    // stays as a bare carrier — the reduction would otherwise produce
    // a deep partially-resolved expression with `semanticMiss` shells
    // because the cycle is broken mid-traversal.
    let is_generic_instantiation =
        matches!(&expr, TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty());

    // The carrier-stop decision is dispatch-layer (demand context),
    // not a projector-side name predicate. The dispatch's
    // `may_reduce_operator(ctx)` predicate is structural and uniform:
    // a userland `MyPick<T,K>` follows the same path as the builtin
    // `Pick<T,K>`, and `Tool<INPUT, OUTPUT>` only carrier-stops when
    // the inner `keyof T` / `Mapped` dispatches enter a non-
    // publication context.
    if is_generic_instantiation
        && crate::meta_resolve::lowered_root_reaches_transitive_cycle(
            query_engine,
            scope_canonical_id,
            &expr,
        )
    {
        return expr;
    }
    let needs_reduction = type_expr_contains_reducible_operator(&expr) || is_generic_instantiation;

    if !needs_reduction {
        return expr;
    }

    // Gates have cleared: re-peek the operator-shape cache. A warm
    // `MaterializeMemoDb` hit on `(scope, expr, mode)` now safely
    // short-circuits the bounded reducer dispatch — package-backed
    // and cycle paths already returned above, so the cached entry's
    // reduced shape is compatible with the projector's published
    // surface.
    if let Some(PeekedShape::Cached(materialized)) =
        peek_member_shape_known(query_engine, scope_canonical_id, &expr, publish_mode)
    {
        return materialized.type_expr;
    }

    // The materialiser propagates the caller's `publish_mode`
    // verbatim into the lower + raise pipeline so the per-prop
    // publication boundary sees the shallower demand at every
    // recursive `Instantiate` / `KeyOf` / `Mapped` dispatch.
    // Hardcoding `ProjectionMode::Expanded` here would silently
    // upgrade `Navigate` callers (`reduce_published_field_types` →
    // `reduce_field_type_expr_with_mode(Navigate)`), and the
    // upgraded path would re-enter `build_key_of` /
    // `build_mapped_type` for inherited helpers like
    // `Partial<EditorOptions>` / `Omit<EmblaOptionsType>` /
    // generic-substituted carriers — emitting per-key
    // `ProjectMember` edges for every enumerated inherited key.
    //
    // The materialiser's cache key is keyed on
    // `(scope, expr, ProjectionReductionContext::published(mode))`
    // (demand-substrate), so the demand-explicit
    // `Published(Navigate)` slot stays disjoint from the implicit
    // `Published(Expanded)` slot — no cache poisoning between
    // per-prop callers and slot/model-binding callers.
    //
    // TypeExpr-start callers that need `Expanded` (slot bindings,
    // model bindings, the `Pick`/`Omit`/`IndexedAccess`/`keyof`
    // paths) enter through [`reduce_field_type_expr`] (the
    // default-`Expanded` overload) and pass `Expanded` here. Callers
    // that explicitly name `Navigate` at
    // `reduce_field_type_expr_with_mode` get the shallower
    // materialisation depth instead.
    let materialized = super::materialize::materialize_component_meta_type_expr_until_stable_full(
        &expr,
        scope_canonical_id,
        publish_mode,
        query_engine,
    );
    let reduced = materialized.type_expr;
    if matches!(&expr, TypeExpr::IndexedAccess { .. })
        && matches!(
            &reduced,
            TypeExpr::Ref {
                type_arguments,
                ..
            } if type_arguments.is_empty()
        )
    {
        let terminal = materialized
            .node_id
            .map(|node_id| {
                super::materialize::reduce_member_value_graph_native_with_context(
                    query_engine.ctx,
                    scope_canonical_id,
                    node_id,
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Expanded,
                    ),
                )
                .type_expr
            })
            .unwrap_or_else(|| {
                query_engine.materialize_member_surface_expr(scope_canonical_id, &reduced, true)
            });
        if !matches!(
            &terminal,
            TypeExpr::Ref {
                type_arguments,
                ..
            } if type_arguments.is_empty()
        ) {
            return terminal;
        }
    }
    reduced
}

/// Resolve a surface member's value to its underlying body for
/// exactness classification. For `DeclRef` carriers (e.g. an
/// unparameterised type alias `MyStr` referenced from a property
/// signature), dispatches `ProjectPath { base: value, path: [],
/// mode: Shallow }` which expands the `DeclRef` to its body. For
/// other variants the value is returned unchanged — `classify_node`
/// already alias-unwraps a single `Alias` hop.
///
/// When `carrier_mode` is set (the member is published as a
/// `Navigate` carrier), an `InstantiationRef` (a generic
/// instantiation such as `Tool<INPUT, OUTPUT>`) is NOT expanded to
/// its `Shallow` object surface. `Shallow` synthesises the one-level
/// object surface — for an interface-bodied generic that breadth-
/// enumerates the instantiated type's members into the audit
/// footprint, which is a Rule-5 (shallow-by-default) violation. A
/// carrier member's exactness IS `ExactSymbolic` (the un-expanded
/// `InstantiationRef` node classifies as symbolic), so skipping the
/// expansion produces the correct exactness without the leak.
/// `DeclRef` (an unparameterised alias such as
/// `type MyStr = string`) is still expanded — that is a single-hop
/// alias unwrap, not an object breadth-enumeration.
///
/// Dep-signature is fanned into every active fact tracer
/// unconditionally so the final-result cache observes the same
/// revalidation surface as the projector's other dispatches.
fn resolve_member_value_for_classification(
    dispatch: &ProjectSemanticDispatch<'_>,
    value: SemanticNodeId,
    carrier_mode: bool,
) -> SemanticNodeId {
    let should_expand =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, value).as_deref() {
            Some(SemanticNodeData::DeclRef { .. }) => true,
            // Carrier-mode: do NOT expand a generic instantiation to
            // its shallow object surface — that would breadth-
            // enumerate the instantiated type's members (a Rule-5
            // shallow-by-default violation). The un-expanded node
            // classifies as `ExactSymbolic`, the correct exactness
            // for a carrier.
            Some(SemanticNodeData::InstantiationRef { .. }) => !carrier_mode,
            _ => false,
        };
    if !should_expand {
        return value;
    }
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: value,
        path: empty_path(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
    match read.value {
        QueryResult::Value(id) => id,
        _ => value,
    }
}
