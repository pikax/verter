//! Materialization core: TypeExpr stabilizer.
//!
//! Owns:
//! - the bounded fixed-point reducer
//!   (`materialize_component_meta_type_expr_until_stable` + `_full`),
//! - the package-backed-root predicate that gates the projector's
//!   reduction decision (`type_expr_has_package_backed_object_like_root`),
//! - the migration helper `lowered_preserve_package_backed_symbolic_refs`
//!   used by the registry-materialise path,
//! - the test-only `MTL_CALL_COUNT` instrumentation that the
//!   eager-entry tests count off.
//!
//! The projector path
//! (`meta_resolve::projectors::reduce_published_field_types` +
//! `reduce_field_type_expr`) is the sole post-projection authority
//! for finalising published field types.

use crate::instant::Instant;

use super::super::dep_signature::emit_dispatch_dep_signature_facts;
use super::super::registry_materialize::preserve_package_backed_symbolic_refs_node;

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_type_expr::TypeExpr {
    materialize_component_meta_type_expr_until_stable_full(
        expr,
        scope_canonical_id,
        mode,
        query_engine,
    )
    .type_expr
}

/// Materialize a `TypeExpr` and return both the result and the
/// producing `SemanticNodeId` + accumulated dep_signature
/// ([`MaterializedTypeExpr`]). Sidecar-capture call sites read
/// `.node_id`; the session merges `.dep_signature` into
/// `ResolvedComponentMetaState.fact_versions` before publish.
///
/// The main entry [`materialize_component_meta_type_expr_until_stable`]
/// remains for callers that need only the `TypeExpr` shell — it
/// delegates here and discards `node_id` / `dep_signature`.
///
/// Materialization flows entirely through dispatch:
/// `shallow_lower_type_expr` → `raise_and_reduce(mode)`. The dispatch
/// covers the substitution-parity surfaces that drive the reducer
/// (Pick<X,K>['member'] indexed access, mapped+conditional `infer P`
/// per-key reduction, method signatures used as `IndexedAccess`
/// bases).
///
/// Per-request memoisation is preserved so repeat queries of the same
/// `(scope, expr, mode)` triple within one component-meta request
/// reuse the prior result instead of re-running the dispatch
/// reduction. Dispatch's own family memo handles cross-request
/// deduplication.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable_full(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::NodeScopeId;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    // Step 6.2 / D22: count every entry into whole-expression
    // materialization. Memo hits + cold builds both increment so the
    // FAIL-FIRST test discriminates the call-ordering contract at the
    // *entry* boundary, not the build closure.
    #[cfg(test)]
    MTL_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // §4.5 items 2-5: per-request memo keyed on `(scope, candidate, mode)`.
    let memo_key = (
        scope_canonical_id.to_string(),
        expr.clone(),
        matches!(mode, crate::semantic_query::ProjectionMode::Navigate),
    );
    #[cfg(test)]
    crate::spike_instrumentation::record_cache_read("materialize_memo");
    if let Some(cached) = query_engine
        .materialize_memo
        .borrow()
        .get(&memo_key)
        .cloned()
    {
        return cached;
    }

    // Step 3 closure: peek ctx-owned MaterializeMemoDb.
    {
        // Loop-5 instrumentation — bump peek for every host-memo
        // read attempt; bump hit only on the cached return path.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PEEKS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let ctx = query_engine.ctx();
        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = ctx.project_type_store().materialize_memo_db();
        if let Some(cached) = host_db.peek(&arc_key, ctx) {
            crate::loop5_instrumentation::MATERIALIZE_MEMO_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            query_engine
                .materialize_memo
                .borrow_mut()
                .insert(memo_key, cached.clone());
            return cached;
        }
    }

    // Step 1.5 thin dispatch wrapper. Build NodeScopeId for the file
    // scope, then lower → raise_and_reduce in the caller's mode.
    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    let ctx = query_engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let env: FxHashMap<String, crate::semantic_query::SemanticNodeId> = FxHashMap::default();
    // Establish ONE tear-free observation of the scope's content
    // identity. The scope's `whole_hash` feeds two distinct consumers
    // that MUST agree:
    //
    //  1. The `NodeScopeId::File` the materialiser lowers the
    //     `TypeExpr` against (built just below) — the lowered value's
    //     semantic identity.
    //  2. The `MaterializeMemoDb` entry's fact-signature self-root
    //     (threaded into the write-through, further down) — the
    //     view-correct SHARED-cache admission gate.
    //
    // Sourcing those from two separate oracles (`shallow_file_state`
    // for the scope id, `authoritative_current_content_hash` for the
    // signature) tears: an edit landing between the two reads roots a
    // value lowered under `H1` on a signature self-rooted at `H2`.
    // `observe_materialize_scope` collapses both onto ONE
    // `Arc<IndexedReady>`: `whole_hash()` is the single source for both
    // the lowering scope and the signature self-root, and the pinned
    // `SyntacticExportSet` parse fact descends from the same artifact.
    // The observation is view-correct — an overlay-bearing
    // `SessionResolverContext` pins the overlay `IndexedReady`, so an
    // overlay-derived memo entry roots on the overlay version (a base
    // request mismatches it rather than reusing it).
    let observed_scope = ctx
        .observe_materialize_scope(scope_canonical_id)
        .expect("materialize scope must have a real indexed scope identity");
    let observed_scope_whole_hash = observed_scope.whole_hash();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash: observed_scope_whole_hash,
        local_scope: None,
    };
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    // R15/F11 — capture the scope-shadowing context
    // once for the materialize → lower pipeline so the dispatch
    // fast-path observes the same shadow set the route extraction
    // path uses.
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
        scope_payload.as_deref(),
    );
    let _us_trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
    if _us_trace {
        eprintln!(
            "[US_LOWER_START] scope={} mode={:?}",
            scope_canonical_id, mode
        );
    }
    let _us_lower_t0 = Instant::now();
    let lowered = dispatch.shallow_lower_type_expr(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        &shadowing,
        &mut substitutions,
        mode,
    );
    let _us_lower_ms = _us_lower_t0.elapsed().as_secs_f64() * 1000.0;
    if _us_trace {
        eprintln!(
            "[US_LOWER_END] scope={} mode={:?} lower_ms={:.1}",
            scope_canonical_id, mode, _us_lower_ms
        );
    }
    let _us_rr_t0 = Instant::now();
    let dispatch_materialized = dispatch.raise_and_reduce(lowered, mode);
    let _us_rr_ms = _us_rr_t0.elapsed().as_secs_f64() * 1000.0;
    if _us_trace {
        eprintln!(
            "[US_RAISE_END] scope={} mode={:?} raise_reduce_ms={:.1}",
            scope_canonical_id, mode, _us_rr_ms
        );
    }

    // Dual-emit dispatch facts into BOTH downstream channels:
    // (1) the legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` drained at
    // `compute_component_meta_state_inner` into `state.fact_versions`,
    // and (2) the `ACTIVE_TRACERS` stack captured by the outer
    // `with_fact_tracer` scope. The bridge helper drops route- and
    // project-generation entries (only `WholeHash` survives the
    // conversion); the dropped entries are R20-only signals with no
    // `FactVersionRef` equivalent.
    emit_dispatch_dep_signature_facts(ctx, &dispatch_materialized.dep_signature);

    let materialized = MaterializedTypeExpr {
        node_id: dispatch_materialized.node_id,
        type_expr: dispatch_materialized.type_expr,
        dep_signature: dispatch_materialized.dep_signature,
    };

    // Step 3 closure: write-through to ctx-owned MaterializeMemoDb.
    {
        // Loop-5 instrumentation — count every publish attempt. The
        // get_or_compute path is a no-op on a concurrent winner but
        // we count the attempt because the bench is single-threaded.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PUBLISHES
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = ctx.project_type_store().materialize_memo_db();
        let captured_value = materialized.clone();
        // Thread the SINGLE tear-free scope observation taken above into
        // the write-through. The signature builder is provenance-pure:
        // it roots the keyed scope on the observation's `whole_hash` and
        // pinned `SyntacticExportSet` parse fact, never on a re-read of
        // current content. The lowering `NodeScopeId` was built from the
        // SAME observation's `whole_hash`, so the memo value and its
        // fact signature root on one identical scope hash — no torn
        // read.
        let captured_scope_observation = observed_scope;
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            // The keyed scope canonical is the entry's self-root, rooted
            // on the observed materialisation-time content version;
            // every canonical the materialisation walk observed (carried
            // on the materialised value's `dep_signature`) is rooted by
            // the fact matching its recorded `DepVersion` so an edit to
            // any contributing file invalidates the memo.
            //
            // The scope's `SyntacticExportSet` parse fact rides on the
            // observation. It is `None` when the observed version's
            // parse-fact registry is not recoverable; the closure then
            // `?`-returns `None` so the entry is not admitted to the
            // shared memo. `engine_fact_signature_for_materialize_memo`
            // also returns `None` when an observed dependency carries a
            // `RouteGeneration` version (route generation has no real
            // validating source). On any `None` here the freshly-
            // computed `materialized` value is still returned to the
            // caller below; only the shared-cache admission is refused.
            let observed_scope_syntactic_export_set =
                captured_scope_observation.syntactic_export_set.clone()?;
            let fact_sig = crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                &captured_scope_observation,
                observed_scope_syntactic_export_set,
                &captured_value.dep_signature,
            )?;
            Some((captured_value, fact_sig))
        });
    }

    query_engine
        .materialize_memo
        .borrow_mut()
        .insert(memo_key, materialized.clone());
    materialized
}

pub(crate) fn type_expr_has_package_backed_object_like_root(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    fn root_name(expr: &verter_type_expr::TypeExpr) -> Option<String> {
        use verter_type_expr::TypeExpr;

        match expr {
            TypeExpr::Parenthesized(inner) => root_name(inner),
            TypeExpr::IndexedAccess { object, .. } => root_name(object),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if matches!(name.as_ref(), "Pick" | "Omit") && type_arguments.len() == 2 => {
                crate::resolver_core::component_meta_registry::component_meta_registry_ref_name(
                    &type_arguments[0],
                )
                .map(str::to_string)
            }
            TypeExpr::Ref { name, .. } => Some(name.to_string()),
            _ => None,
        }
    }

    let Some(root_name) = root_name(expr) else {
        return false;
    };

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    // Issue #11 / route the package-backed classification
    // through `WorkspaceRead::is_package_backed` (NOT a path-substring
    // check on `node_modules`). The realpath-based classification
    // correctly handles pnpm-symlinks and workspace-packages-inside-
    // node_modules.
    if !query_engine
        .ctx
        .workspace_is_package_backed(declaration_scope.as_str())
    {
        return false;
    }

    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        return true;
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(&body)
        })
}

/// Migration helper. Lowers `materialized` and `raw`
/// TypeExpr inputs to Navigate-mode `SemanticNodeId`s, dispatches to
/// J4's graph-native [`preserve_package_backed_symbolic_refs_node`],
/// and raises the result back to TypeExpr.
///
/// Returns `materialized.clone()` (matches the deleted TypeExpr
/// predicate's `_ => materialized.clone()` arm) when either lowering
/// fails or the raise back to TypeExpr fails — preserves existing
/// behaviour for shapes the dispatcher cannot lower deterministically.
pub(crate) fn lowered_preserve_package_backed_symbolic_refs(
    materialized: &verter_type_expr::TypeExpr,
    raw: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_type_expr::TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let ctx = engine.ctx;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let Some(materialized_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        materialized,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let Some(raw_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        raw,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let preserved_node =
        preserve_package_backed_symbolic_refs_node(ctx, materialized_node, raw_node, 0);
    if preserved_node == materialized_node {
        return materialized.clone();
    }
    dispatch
        .raise_node_to_type_expr(preserved_node)
        .unwrap_or_else(|| materialized.clone())
}

/// Test-only call counter for `materialize_component_meta_type_expr_until_stable`.
/// Incremented at function entry — memo hits and cold builds both
/// count, since the counter discriminates the *entry* invariant: did
/// the caller route through whole-expression materialization at all?
#[cfg(test)]
pub(crate) static MTL_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Test accessor for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn mtl_call_count_for_tests() -> usize {
    MTL_CALL_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Test reset for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_mtl_call_count_for_tests() {
    MTL_CALL_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}
