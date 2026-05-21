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
//! 3. `dispatch.execute_read(SemanticQueryKey::ProjectPath { base, path: [], mode: Shallow })`
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

pub(crate) mod emits;
pub(crate) mod exposed;
pub(crate) mod model;
pub(crate) mod options;
pub(crate) mod props;
pub(crate) mod slots;

pub(crate) use emits::project_emits;
pub(crate) use exposed::project_exposed;
pub(crate) use model::project_model;
pub(crate) use options::project_options;
pub(crate) use props::project_props;
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

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                let fields = project_props(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
                merge_projected_fields_by_name(&mut evaluated_types.props, fields);
            }
            AnalyzedMacroKind::DefineEmits => {
                let fields = project_emits(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
                merge_projected_fields_by_name(&mut evaluated_types.emits, fields);
            }
            AnalyzedMacroKind::DefineSlots => {
                // Slot-shape projection is consumed by the
                // slot-binding-graph synthesis layer which
                // shares the same dispatch primitives; running the
                // projector here populates the diagnostic stream and
                // primes the dispatch family memo.
                let _ = project_slots(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
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
                let _ = project_model(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
            AnalyzedMacroKind::DefineExpose => {
                let _ = project_exposed(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                );
            }
            AnalyzedMacroKind::DefineOptions => {
                let _ = project_options(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
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
//    `MaterializeMemoDb` keyed on `(scope, expr, mode)`. The peek
//    re-emits the cached entry's `fact_dep_signature` into the active
//    fact tracer (mirroring the `MemberShapeCacheDb::peek` protocol) so
//    the cm-result cache validation invariants are preserved.
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
            // or generic instantiation: consult `ShapeCacheDb` only
            // (Block 6.i universal cache). Does NOT consult RouteDb/
            // OwnerImportSurfaceDb (those would rebuild HostStoreView
            // and force the very cost driver Block 6.g closed).
            let ctx: &dyn ResolverContext = query_engine.ctx;
            let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
                Arc::<str>::from(scope_canonical_id),
                Arc::new(expr.clone()),
                mode,
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
    let type_args: Arc<[SemanticNodeId]> = match dispatch.lower_type_expr_in_scope_with_mode(
        file,
        parsed_arg,
        ProjectionMode::Navigate,
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
        owner: owner.clone(),
        macro_index,
        macro_kind,
        type_args,
        mode: ProjectionMode::Navigate,
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

    Some(payload_node)
}

/// Resolve the empty-path `ProjectPath` surface for a payload node.
///
/// Returns the surface node on success. On `Recursive` or `Error`,
/// appends a diagnostic to `diag_sink` and returns `None`.
pub(crate) fn resolve_payload_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    let surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: payload_node,
        path: empty_path(),
        mode: ProjectionMode::Shallow,
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

/// Peek-before-raise per-member helper.
///
/// Wraps the cold compute path for one `(scope, member_value, mode)`
/// triple around the host-owned
/// [`crate::component_meta_caches::MemberShapeCacheDb`]. The contract:
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
    let key = crate::component_meta_caches::ShapeCacheKey::semantic_node_whole(
        Arc::<str>::from(scope_canonical_id),
        member_value,
        mode,
    );

    // (1) Peek FIRST — warm path pays zero raise/gate cost. The cached
    // entry's dep_signature must be re-emitted into the active fact
    // tracer + dispatch dep-signature accumulator so the request's
    // dep set sees the same facts the cold compute emitted.
    // Block 6.i universal cache (replaces `MemberShapeCacheDb`).
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
    let (route_is_package_backed, package_backed_fence) =
        super::materialize::type_expr_has_package_backed_object_like_root_with_fence(
            &raised,
            scope_canonical_id,
            query_engine,
        );
    if route_is_package_backed {
        // Gate short-circuit: the published shape stays as the
        // raised carrier (package-backed roots are shallow). Block
        // 6.i universal caching: admit so sibling members of the
        // same package-backed parent (e.g. siblings of `Foo['a']`
        // when `Foo` is from a package) reuse this verdict at peek
        // time rather than re-running the package-backed predicate.
        // F1: thread the gate's cross-file fence into the admit so
        // edits to the package-backing declaration file invalidate.
        let value = MaterializedTypeExpr {
            node_id: Some(member_value),
            type_expr: raised,
            dep_signature: package_backed_fence,
        };
        return admit_member_shape_if_possible(ctx, &key, value);
    }
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
            // Block 6.i universal caching: admit so subsequent peeks
            // skip the cycle BFS.
            // F1: combine both gate fences (package-backed + cycle BFS)
            // so the admit's `fact_dep_signature` invalidates on edits
            // to any visited declaration file.
            let combined_fence =
                combine_dep_signatures(&package_backed_fence, &fence, scope_canonical_id);
            let value = MaterializedTypeExpr {
                node_id: Some(member_value),
                type_expr: raised,
                dep_signature: combined_fence,
            };
            return admit_member_shape_if_possible(ctx, &key, value);
        }
        fence
    } else {
        Arc::from(Vec::new())
    };
    let needs_reduction =
        type_expr_contains_reducible_operator(&raised) || is_generic_instantiation;
    // F1: combined gate fence threaded through every remaining admit
    // path so the cache entries do not self-root on the scope file
    // only.
    let gate_fence =
        combine_dep_signatures(&package_backed_fence, &cycle_fence, scope_canonical_id);
    if !needs_reduction {
        // Block 6.i universal-caching invariant: a shape that
        // resolves to a non-reducible TypeExpr (primitive / literal /
        // bare alias / closed object / function / union / intersection
        // without operator nodes) is a STABLE shape — admit it so
        // sibling members hitting the same `SurfaceMember.value` (or
        // the same `(scope, node, mode)` triple from a downstream
        // call) short-circuit at peek time.
        let value = MaterializedTypeExpr {
            node_id: Some(member_value),
            type_expr: raised,
            dep_signature: gate_fence,
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
    // Block 6.i universal-caching invariant: when the peek returns
    // `Leaf` / `BareCarrier` we still admit a SemanticNode-subject
    // entry to the cache via `admit_computed` so subsequent member
    // peeks short-circuit at peek time.
    if let Some(peeked) = peek_member_shape_known(query_engine, scope_canonical_id, &raised, mode) {
        match peeked {
            PeekedShape::Leaf(leaf) => {
                let value = MaterializedTypeExpr {
                    node_id: Some(member_value),
                    type_expr: leaf,
                    dep_signature: gate_fence,
                };
                return admit_member_shape_if_possible(ctx, &key, value);
            }
            PeekedShape::BareCarrier { .. } => {
                let value = MaterializedTypeExpr {
                    node_id: Some(member_value),
                    type_expr: raised,
                    dep_signature: gate_fence,
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
    let materialized = super::materialize::reduce_member_value_graph_native(
        ctx,
        scope_canonical_id,
        member_value,
        mode,
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
        let fact_sig =
            crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                &scope_obs,
                parse_fact,
                &materialized_for_closure.dep_signature,
            )?;
        Some((materialized_for_closure, fact_sig))
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
/// Block 6.i universal-caching invariant — every successful
/// `(node, scope, mode)` shape compute admits so sibling members and
/// future peeks return the cached value rather than re-paying the
/// raise + gate cost.
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
/// Block 6.i universal-caching invariant: every successful shape
/// compute admits, regardless of how cheap the compute was.
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
    };
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
        Arc::<str>::from(scope_canonical_id),
        Arc::new(expr.clone()),
        mode,
    );
    let _ = admit_member_shape_if_possible(ctx, &key, value);
}

fn admit_member_shape_if_possible(
    ctx: &dyn ResolverContext,
    key: &crate::component_meta_caches::ShapeCacheKey,
    value: crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    let scope = key.subject.scope_canonical().clone();
    let observed_scope = ctx.observe_materialize_scope(scope.as_ref());
    let value_for_closure = value.clone();
    let cache = ctx.project_type_store().shape_cache_db();
    let admitted = cache.get_or_compute(key, ctx, move || {
        let scope_obs = observed_scope?;
        let parse_fact = scope_obs.syntactic_export_set.clone()?;
        let fact_sig =
            crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                &scope_obs,
                parse_fact,
                &value_for_closure.dep_signature,
            )?;
        Some((value_for_closure, fact_sig))
    });
    admitted.unwrap_or(value)
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
) -> ExpandedField {
    let ctx: &dyn ResolverContext = query_engine.ctx;
    // Exactness classification is independent of the member's reduced
    // TypeExpr; it walks the member's resolved-value graph. Keep it
    // isolated in its own dispatch scope so the peek-before-raise
    // contract for the type reduction is not coupled to a TypeExpr
    // raise that exactness does not need.
    let exactness = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let resolved_value = resolve_member_value_for_classification(&dispatch, member.value);
        classify_node(&dispatch, resolved_value)
    };
    // Peek the per-member graph-native materialiser cache BEFORE any
    // `raise_node_to_type_expr(member.value)` call. Warm hits return
    // the cached `MaterializedTypeExpr` without paying the raise cost
    // or the shallow-gate cost; cold misses raise once, run the
    // gates, then dispatch the graph-native reducer + admit.
    let materialized = member_shape_peek_or_compute(
        query_engine,
        scope_canonical_id,
        member.value,
        ProjectionMode::Expanded,
    );
    let r#type = materialized.type_expr;
    debug_assert_eq!(
        shallow_type_expr.is_some(),
        shallow_type_expr_scope.is_some(),
        "ExpandedField (surface member `{}`) shallow_type_expr/shallow_type_expr_scope pairing violated",
        member.name.as_ref(),
    );
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
    // Block 6.i universal-caching invariant: when the peek returns
    // `Leaf` / `BareCarrier`, ADMIT a TypeExpr-subject entry into
    // `ShapeCacheDb` so subsequent TypeExpr-start callers (e.g.
    // `materialize_component_meta_type_expr_until_stable_full` peek
    // path) hit at peek time. Peek-time admission is cheap (no raise
    // / no reducer dispatch); the cache write is the universal-
    // caching contract.
    if let Some(peeked) = peek_member_shape_known(
        query_engine,
        scope_canonical_id,
        &expr,
        ProjectionMode::Expanded,
    ) {
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
                    ProjectionMode::Expanded,
                    leaf.clone(),
                    Arc::from(Vec::new()),
                );
                return leaf;
            }
            PeekedShape::BareCarrier { .. } => {
                // F1: BareCarrier admission is structural — a `Ref`
                // with empty `type_arguments`. The shape is the
                // carrier name itself; no cross-file work was done.
                // Empty dep_signature is correct; scope-self-rooting
                // is sufficient.
                admit_type_expr_shape_if_possible(
                    query_engine.ctx,
                    scope_canonical_id,
                    &expr,
                    ProjectionMode::Expanded,
                    expr.clone(),
                    Arc::from(Vec::new()),
                );
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
    if let Some(PeekedShape::Cached(materialized)) = peek_member_shape_known(
        query_engine,
        scope_canonical_id,
        &expr,
        ProjectionMode::Expanded,
    ) {
        return materialized.type_expr;
    }

    // Run the bounded fixed-point reducer from the consumer's scope.
    // The dispatch's lower → raise_and_reduce pipeline carries
    // imported declarations through their prepared bodies via the
    // shared resolver caches, so the consumer scope is sufficient
    // for cross-file alias resolution within the explicitly-walked
    // path. Cross-scope re-resolution must come from the consumer's
    // own walk, not from a projector retry that would cross the
    // shallow boundary on alias bodies the consumer never asked
    // about.
    super::materialize::materialize_component_meta_type_expr_until_stable(
        &expr,
        scope_canonical_id,
        ProjectionMode::Expanded,
        query_engine,
    )
}

/// Run the shared field-type reducer over every published surface in
/// `evaluated_types` so consumers see the same finalised shapes the
/// per-macro projectors already publish for `props` / `emits`.
///
/// The slot-binding graph and the parser-side `bindings` synthesis
/// publish `ExpandedField`s whose `r#type` is the raised raw surface —
/// they do not run reduction inline because the slot binding graph's
/// dispatch only enumerates surface members. This pipeline step
/// finalises those rows by routing each through
/// [`reduce_field_type_expr`], which is the same primitive
/// [`surface_member_to_expanded_field`] uses inside the projectors.
///
/// This is the single post-projection authority for finalising
/// `evaluated_types` field shapes; there is no second resolver, no
/// member-route surface synthesis, and no separate cache. All
/// reduction work flows through
/// `materialize_component_meta_type_expr_until_stable` and the
/// dispatch-owned semantic memos it consults.
pub(crate) fn reduce_published_field_types(
    scope_canonical_id: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    use crate::meta_resolve::compare_type_expr_improvement;
    use rustc_hash::FxHashMap;

    let mut finalized_prop_types: FxHashMap<String, TypeExpr> = FxHashMap::default();
    for field in evaluated_types.props.iter_mut() {
        // Typed-IR-Only Resolver Rule: when the raised value-node form
        // reduces to an unresolved shape (e.g. `Mapped { source: Unknown }`
        // from a `Partial<X>` / `Required<X>` expansion where the
        // semantic graph could not lift the mapped operator), prefer
        // the analyzer-populated `shallow_type_expr` — the bare per-prop
        // typed form lowered at OXC visit time. The typed sidecar
        // replaces the legacy raw-annotation reparse fallback; no
        // source-text reparse runs at this site.
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        let mut reduced = reduce_field_type_expr(query_engine, scope_canonical_id, raised);

        if let Some(shallow) = field.shallow_type_expr.as_ref() {
            if !matches!(shallow, TypeExpr::Unknown { .. })
                && compare_type_expr_improvement(shallow, &reduced)
            {
                let shallow_reduced =
                    reduce_field_type_expr(query_engine, scope_canonical_id, shallow.clone());
                if compare_type_expr_improvement(&shallow_reduced, &reduced) {
                    reduced = shallow_reduced;
                }
            }
        }

        finalized_prop_types.insert(field.name.clone(), reduced.clone());
        field.r#type = reduced;
    }
    // Back-sync the finalised prop type into the macro-shape mirror
    // on `evaluated_types.define_props`. Producers
    // (`produce_one_macro_object_shape`) populate define_props with
    // the pre-reduction shape; consumers reading the macro shapes
    // (e.g. `evaluated.define_props[..].result.value.properties[..]`)
    // expect the same finalised type the published `props` field
    // carries.
    for define_props in evaluated_types.define_props.iter_mut() {
        for property in define_props.result.value.properties.iter_mut() {
            if let Some(finalised) = finalized_prop_types.get(property.name.as_str()) {
                property.ty = finalised.clone();
            }
        }
    }
    for field in evaluated_types.emits.iter_mut() {
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    }
    for field in evaluated_types.slot_bindings.iter_mut() {
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    }
    for field in evaluated_types.bindings.iter_mut() {
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        field.r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    }
}

/// Does `expr` contain any operator-shape node that the bounded
/// fixed-point reducer should resolve?
///
/// Returns `true` when the expression carries an `IndexedAccess`,
/// `KeyOf`, `TypeOf`, `Conditional`, `Mapped`, or `Infer` anywhere
/// in its tree. For shells that only contain primitives, literals,
/// `Ref`s (to parameterised aliases), `Object`s, `Function`s,
/// `Array`s, `Tuple`s, `Union`s, `Intersection`s, `TypeParameter`s,
/// `RecursiveRef`s, or `Unknown`s, the predicate returns `false` so
/// the symbolic-route preservation contract holds.
pub(crate) fn type_expr_contains_reducible_operator(expr: &TypeExpr) -> bool {
    use verter_type_expr::ObjectMember;

    match expr {
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_reducible_operator(inner)
        }
        TypeExpr::Array { element, .. } => type_expr_contains_reducible_operator(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|el| type_expr_contains_reducible_operator(&el.ty)),
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(type_expr_contains_reducible_operator)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_contains_reducible_operator(&p.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_reducible_operator(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_reducible_operator(&sig.key_type)
                    || type_expr_contains_reducible_operator(&sig.value_type)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                f.parameters
                    .iter()
                    .any(|p| type_expr_contains_reducible_operator(&p.ty))
                    || f.return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
        }),
        TypeExpr::Function(f) => {
            f.parameters
                .iter()
                .any(|p| type_expr_contains_reducible_operator(&p.ty))
                || f.return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_reducible_operator)
        }
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Unknown { .. } => false,
    }
}

/// Resolve a surface member's value to its underlying body for
/// exactness classification. For `DeclRef` carriers (e.g. an
/// unparameterised type alias `MyStr` referenced from a property
/// signature), dispatches `ProjectPath { base: value, path: [],
/// mode: Shallow }` which expands the `DeclRef` to its body. For
/// other variants the value is returned unchanged — `classify_node`
/// already alias-unwraps a single `Alias` hop.
///
/// Dep-signature is fanned into every active fact tracer
/// unconditionally so the final-result cache observes the same
/// revalidation surface as the projector's other dispatches.
fn resolve_member_value_for_classification(
    dispatch: &ProjectSemanticDispatch<'_>,
    value: SemanticNodeId,
) -> SemanticNodeId {
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, value).as_deref() {
        Some(SemanticNodeData::DeclRef { .. })
        | Some(SemanticNodeData::InstantiationRef { .. }) => {
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: value,
                path: empty_path(),
                mode: ProjectionMode::Shallow,
            });
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            match read.value {
                QueryResult::Value(id) => id,
                _ => value,
            }
        }
        _ => value,
    }
}
