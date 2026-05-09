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
//! All `dispatch.execute_read` calls must accumulate dep-signature via
//! `accumulate_dispatch_dep_signature` so the final-result cache can
//! revalidate on warm hits. Cycle and error branches publish a
//! `MacroExpansionDiagnostics` envelope into `diag_sink` (per §7.5
//! silent-miss prevention).
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
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember,
};
use crate::types::FileAnalysisSnapshot;

use super::dep_signature::accumulate_dispatch_dep_signature;
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
/// For each projector field, looks up the target by `name`:
/// - If a matching name exists, the projector publishes the
///   dispatch-resolved surface; the parser-side entry was populated
///   by `evaluate_types` before the projector ran. When the
///   projector's surface is the same shape or a strict improvement
///   (more structural detail / fewer symbolic carriers), the
///   projector wins. When the parser-side entry is strictly more
///   concrete (e.g. it already resolved a recursive alias body that
///   the dispatch path returned as a bare `Ref` because of cycle
///   truncation), the parser-side shape stays and only the metadata
///   fields the projector owns (`raw_type`, `optional`, `exactness`,
///   execution status, diagnostics) merge in.
/// - If no matching name exists, the projector field is appended.
///
/// This keeps any parser-side fields the projector did NOT produce
/// (e.g., entries from prop annotations that the dispatch path did
/// not surface) AND prevents projector regressions for shapes the
/// parser-side path resolved more concretely (recursive alias bodies,
/// re-export chains preserved by the parser-side resolver, etc.).
fn merge_projected_fields_by_name(
    target: &mut Vec<verter_semantic::analysis::type_expand::ExpandedField>,
    projected: Vec<verter_semantic::analysis::type_expand::ExpandedField>,
) {
    use crate::meta_resolve::compare_type_expr_improvement;

    for field in projected {
        if let Some(existing) = target.iter_mut().find(|t| t.name == field.name) {
            if std::env::var("VERTER_PROJECTOR_MERGE_TRACE").is_ok() {
                eprintln!(
                    "[MERGE] name={} existing={:?} projected={:?}",
                    field.name, existing.r#type, field.r#type
                );
            }
            if compare_type_expr_improvement(&field.r#type, &existing.r#type) {
                *existing = field;
            } else if compare_type_expr_improvement(&existing.r#type, &field.r#type) {
                existing.raw_type = field.raw_type;
                existing.optional = field.optional;
                existing.exactness = field.exactness;
                existing.execution_status = field.execution_status;
                existing.diagnostics = field.diagnostics;
            } else {
                *existing = field;
            }
        } else {
            target.push(field);
        }
    }
}

/// Top-level driver that dispatches every type-based macro in the
/// snapshot through its per-kind projector and writes the resulting
/// fields into `evaluated_types`.
///
/// This is the replacement for the legacy macro-shape walker + per-
/// field rescue cascade enrichment pipeline. The driver:
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
    accumulate_dispatch_dep_signature(&payload_read.dep_signature);
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
    accumulate_dispatch_dep_signature(&surface_read.dep_signature);
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
) -> ExpandedField {
    let ctx: &dyn ResolverContext = query_engine.ctx;
    let (raised, exactness) = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let resolved_value = resolve_member_value_for_classification(&dispatch, member.value);
        let raised = dispatch
            .raise_node_to_type_expr(member.value)
            .unwrap_or(TypeExpr::Unknown { raw: String::new() });
        let exactness = classify_node(&dispatch, resolved_value);
        (raised, exactness)
    };
    let r#type = reduce_field_type_expr(query_engine, scope_canonical_id, raised);
    ExpandedField {
        name: member.name.as_ref().to_string(),
        r#type,
        raw_type,
        optional: member.optional,
        exactness,
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
    }
}

/// Drive the shared field-type reduction used by every projector and
/// by [`reduce_published_field_types`] on slot bindings, model bindings,
/// and any leftover parser-side fields.
///
/// The reduction has two stages:
///
/// 1. **Operator collapse** — when `expr` contains any
///    `IndexedAccess` / `KeyOf` / `TypeOf` / `Conditional` /
///    `Mapped` / `Infer` shape AND the route's root is not a
///    package-backed object surface, the bounded fixed-point reducer
///    [`materialize_component_meta_type_expr_until_stable`] runs in
///    `Expanded` mode. Nested chains
///    (`Pick<Foo,'outer'>['outer']['inner']`) collapse to concrete
///    leaves; symbolic Refs to parameterised aliases stay symbolic;
///    package-backed indexed accesses stay symbolic per the route-
///    preservation contract.
/// 2. **Symbolic-Ref body lookup** — when the reduction yields a bare
///    `Ref { name, type_arguments }` whose declaration body is itself
///    not a non-object surface (i.e. an alias to a primitive / object
///    / function shape) AND the body would benefit from projection,
///    the reducer is rerun against an `IndexedAccess`-on-Ref shell
///    (matching the rescue's "imported alias body" recovery).
///    Otherwise the bare Ref is the final shape (consumers re-resolve
///    by name through the registry).
///
/// Generic substitutions, dep-signature accumulation, fence-validated
/// publication, and dispatch fence diagnostics all flow through
/// `materialize_component_meta_type_expr_until_stable` — there is no
/// separate cache, scope, or budget here.
pub(crate) fn reduce_field_type_expr(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: TypeExpr,
) -> TypeExpr {
    use crate::meta_resolve::expr_needs_projection_rescue;
    use crate::meta_resolve::select_imported_materialization_scope;

    let route_is_package_backed = super::materialize::type_expr_has_package_backed_object_like_root(
        &expr,
        scope_canonical_id,
        query_engine,
    );
    if route_is_package_backed {
        return expr;
    }

    // Reduction triggers when (a) the expression carries an
    // operator-shape node (`IndexedAccess`/`KeyOf`/`TypeOf`/
    // `Conditional`/`Mapped`/`Infer`), or (b) the expression's root
    // is a bare `Ref` whose declaration body would benefit from
    // expansion (utility instantiations like `Pick<X,K>`, aliases
    // resolving to non-object surfaces, etc). The latter is
    // detected by `expr_needs_projection_rescue` which inspects the
    // declaration body via the dispatch primitives — its cycle
    // guard prevents runaway recursion on recursive aliases like
    // `TreeNode`. We restrict (b) to bare-`Ref` roots so a Union
    // whose individual branches happen to be utility wrappers
    // (`boolean | Omit<X, K>`) keeps the wrapper symbolic — only
    // the Union root's own non-object surface check is consulted,
    // not each branch's body shape.
    let needs_reduction = type_expr_contains_reducible_operator(&expr)
        || (matches!(&expr, TypeExpr::Ref { .. })
            && expr_needs_projection_rescue(query_engine, scope_canonical_id, &expr));

    if !needs_reduction {
        return expr;
    }

    // Run the bounded fixed-point reducer from the consumer's scope.
    // The dispatch's lower → raise_and_reduce pipeline carries
    // imported declarations through their prepared bodies via the
    // shared resolver caches, so the consumer scope is sufficient
    // for cross-file alias resolution.
    let stable = super::materialize::materialize_component_meta_type_expr_until_stable(
        &expr,
        scope_canonical_id,
        ProjectionMode::Expanded,
        query_engine,
    );

    // Cross-scope retry: if the consumer-scope reduction didn't
    // produce an improvement, try the imported declaration's scope.
    // This matches the rescue's `select_imported_materialization_scope`
    // fallback for routes whose root lives in another file (e.g.
    // imported alias bodies that reference symbols defined alongside
    // the alias).
    if !crate::meta_resolve::compare_type_expr_improvement(&stable, &expr) {
        if let Some(imported_scope) =
            select_imported_materialization_scope(&expr, scope_canonical_id, query_engine)
        {
            let cross_scope = super::materialize::materialize_component_meta_type_expr_until_stable(
                &expr,
                imported_scope.as_str(),
                ProjectionMode::Expanded,
                query_engine,
            );
            if crate::meta_resolve::compare_type_expr_improvement(&cross_scope, &expr) {
                return cross_scope;
            }
        }
    }

    stable
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
        let raised = std::mem::replace(&mut field.r#type, TypeExpr::Unknown { raw: String::new() });
        let mut reduced = reduce_field_type_expr(query_engine, scope_canonical_id, raised.clone());

        // Raw-annotation fallback: when the published surface is the
        // raised value-node form and that form is strictly worse than
        // the parser-side `raw_type` annotation (e.g. an unresolved
        // `Mapped { source: Unknown }` shell from a Partial/Required
        // expansion), parse the annotation and prefer it. The
        // annotation text is the authoritative per-prop string the
        // analyzer surfaced through the macro-shape path.
        if let Some(raw_text) = field.raw_type.as_deref() {
            let raw_parsed =
                verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_text);
            if !matches!(raw_parsed, TypeExpr::Unknown { .. })
                && compare_type_expr_improvement(&raw_parsed, &reduced)
            {
                let raw_reduced =
                    reduce_field_type_expr(query_engine, scope_canonical_id, raw_parsed);
                if compare_type_expr_improvement(&raw_reduced, &reduced) {
                    reduced = raw_reduced;
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
    use verter_semantic::analysis::type_expr::ObjectMember;

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
/// Dep-signature is accumulated unconditionally so the final-result
/// cache observes the same revalidation surface as the projector's
/// other dispatches.
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
            accumulate_dispatch_dep_signature(&read.dep_signature);
            match read.value {
                QueryResult::Value(id) => id,
                _ => value,
            }
        }
        _ => value,
    }
}
