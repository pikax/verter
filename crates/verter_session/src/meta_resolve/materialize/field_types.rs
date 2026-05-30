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

pub(crate) fn type_expr_materializer_context(
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate) {
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(mode)
    } else {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    }
}

/// The EXACT [`ProjectionReductionContext`] the whole-expression
/// materialiser lowers + reduces `expr` under, given the caller's
/// `mode`.
///
/// gap1: the `ShapeCacheDb` peek/publish slot MUST be keyed by this
/// context so a cache entry's stored value (reduced under this context)
/// is only ever served to a consumer that lowered under the SAME
/// context. The whole-expression materialiser has TWO reduction
/// contexts for a `Navigate` caller:
///
/// - a `Navigate` caller whose `expr` root is itself a *published*
///   operator (`Pick`/`Omit`/`IndexedAccess`/...) reduces under
///   `Published(Navigate)` — the explicit narrowing IS consumer demand,
///   so the operator reduces path-precisely even at the shallow
///   publication boundary; and
/// - every other `Navigate` caller reduces under
///   `StructuralTransit(Navigate)` (operators carrier-stop).
///
/// `Expanded` / other modes reduce under `Published(mode)`.
///
/// Keying the slot on a bare `published(mode)` while reducing under
/// `StructuralTransit(mode)` stored a transit-lowered value at a
/// published-keyed slot and served it to a published consumer — the
/// gap1 poisoning. This helper is the single source for both the
/// reduction and the cache key.
pub(crate) fn type_expr_materialize_reduction_context(
    expr: &verter_type_expr::TypeExpr,
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate)
        && type_expr_root_is_published_operator(expr)
    {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    } else {
        type_expr_materializer_context(mode)
    }
}

fn type_expr_root_is_published_operator(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_root_is_published_operator(inner),
        TypeExpr::Ref { .. } => {
            // References that survive parser lowering are declared
            // surface roots. Builtin broad mapped carriers are handled
            // by the `Mapped` arm below after dispatch lowering.
            true
        }
        TypeExpr::Mapped { value, .. } => {
            // Builtin broad object modifiers lower to identity mapped
            // carriers with an opaque/miss placeholder value. Keep
            // those as carriers at Navigate depth; publish mapped
            // types that carry an author-visible value expression
            // (`T[K]`, `string`, `Record<...>`, etc.).
            !matches!(
                value.as_ref(),
                TypeExpr::Unknown { raw } if raw == "semanticMiss"
            )
        }
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::TypeOf(_) => true,
        _ => false,
    }
}

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
/// context-aware shallow lowering -> context-aware graph reduction. The
/// dispatch covers the substitution-parity surfaces that drive the reducer
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

    // gap1: the host-owned `ShapeCacheDb` is THE materialiser cache.
    // The former request-local `materialize_memo` keyed on
    // `(scope, expr, navigate_bool)` was a SECOND authoritative cache
    // (a host-owned-cache-principle violation) AND keyed only on a
    // mode-collapsed `navigate_bool` — distinct reduction contexts over
    // the same `(scope, expr)` collided onto one cell. It is gone.
    //
    // The cache slot is keyed by the EXACT
    // [`ProjectionReductionContext`] the reduction below actually runs
    // under (`reduction_context`, computed here so the peek key and the
    // value share one identity). Keying on `published(mode)` while
    // reducing under `StructuralTransit(mode)` (the `Navigate` case)
    // poisoned a published consumer with a transit-lowered value.
    let reduction_context = type_expr_materialize_reduction_context(expr, mode);

    // Peek the universal ShapeCacheDb (TypeExpr subject,
    // whole-subject demand under the exact reduction context).
    {
        // Loop-5 instrumentation — bump peek for every host-memo
        // read attempt; bump hit only on the cached return path.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PEEKS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let ctx = query_engine.ctx();
        let cache_key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            reduction_context,
        );
        let host_db = ctx.project_type_store().shape_cache_db();
        if let Some(cached) = host_db.peek(&cache_key, ctx) {
            crate::loop5_instrumentation::MATERIALIZE_MEMO_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    //
    // A `None` observation is a legitimate outcome (a session
    // tombstone, an evicted / unloaded scope, or no recoverable
    // artifact): the materialiser still runs and returns a value, but
    // there is no view-correct scope identity to self-root a shared
    // `MaterializeMemoDb` entry with, so the shared-cache write-through
    // below is skipped. The lowering then degrades exactly as the
    // missing-`scope_payload` path already does — it sources the
    // lowering `NodeScopeId`'s `whole_hash` from the scope's surviving
    // `shallow_file_state` content version, NEVER a fabricated all-zero
    // hash. When the scope has neither an observation nor a surviving
    // shallow state there is genuinely no scope identity to lower
    // against, so the materialiser returns the input expression
    // unchanged — the no-op result the surrounding code already
    // tolerates.
    let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
    let lowering_scope_whole_hash = match observed_scope.as_ref() {
        Some(observation) => Some(observation.whole_hash()),
        None => ctx
            .shallow_file_state(scope_canonical_id)
            .map(|state| state.whole_hash),
    };
    let Some(observed_scope_whole_hash) = lowering_scope_whole_hash else {
        return MaterializedTypeExpr {
            node_id: None,
            type_expr: expr.clone(),
            dep_signature: Arc::from(Vec::new()),
            cache_suppress: false,
        };
    };
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
    // `reduction_context` was computed at function entry (gap1) so the
    // ShapeCacheDb peek/publish key shares one identity with the value.
    let _us_lower_t0 = Instant::now();
    let lowered = dispatch.shallow_lower_type_expr_with_context(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        &shadowing,
        &mut substitutions,
        reduction_context,
    );
    let _us_lower_ms = _us_lower_t0.elapsed().as_secs_f64() * 1000.0;
    if _us_trace {
        eprintln!(
            "[US_LOWER_END] scope={} mode={:?} lower_ms={:.1}",
            scope_canonical_id, mode, _us_lower_ms
        );
    }
    let _us_rr_t0 = Instant::now();
    let dispatch_materialized = dispatch.raise_and_reduce_with_context(lowered, reduction_context);
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
        cache_suppress: dispatch_materialized.cache_suppress,
    };

    // Step 3 closure: write-through to ctx-owned MaterializeMemoDb.
    //
    // Gated on a `Some` scope observation. A `None` observation (a
    // session tombstone, an evicted / unloaded scope, or no recoverable
    // artifact) has no view-correct scope identity to self-root a
    // shared `MaterializeMemoDb` entry with, so shared-cache admission
    // is skipped entirely — the freshly-computed `materialized` value
    // is still returned to the caller below. The lowering above already
    // degraded to the scope's surviving `shallow_file_state` version
    // for that case; admitting a shared entry rooted on that lowering
    // hash without the observation's pinned `SyntacticExportSet` parse
    // fact would be a mis-rooted write.
    //
    // Additional gate: `cache_suppress=true` on the freshly materialized
    // value means a downstream `dispatch.execute_read(...)` exhausted the
    // projection-op budget (or returned another fatal `QueryError`). The
    // partial outcome must NOT warm the shared `ShapeCacheDb` slot —
    // admitting it would poison subsequent identical-key lookups against
    // the same scope+expr+mode triple. The freshly-computed value is
    // still returned to the caller; only the shared-cache admission is
    // refused.
    if !materialized.cache_suppress {
        if let Some(captured_scope_observation) = observed_scope {
            // Loop-5 instrumentation — count every publish attempt. The
            // get_or_compute path is a no-op on a concurrent winner but
            // we count the attempt because the bench is single-threaded.
            crate::loop5_instrumentation::MATERIALIZE_MEMO_PUBLISHES
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            let cache_key =
                crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
                    std::sync::Arc::<str>::from(scope_canonical_id),
                    std::sync::Arc::new(expr.clone()),
                    reduction_context,
                );
            let host_db = ctx.project_type_store().shape_cache_db();
            let captured_value = materialized.clone();
            // The SINGLE tear-free scope observation taken above is threaded
            // into the write-through. The signature builder is
            // provenance-pure: it roots the keyed scope on the observation's
            // `whole_hash` and pinned `SyntacticExportSet` parse fact, never
            // on a re-read of current content. The lowering `NodeScopeId`
            // was built from the SAME observation's `whole_hash`, so the
            // memo value and its fact signature root on one identical scope
            // hash — no torn read.
            let _ = host_db.get_or_compute(&cache_key, ctx, move || {
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
    }

    // gap1: no request-local write-through. The host-owned
    // `ShapeCacheDb` get_or_compute above is the SOLE materialiser
    // cache; the same request's later reduce calls re-peek it (a warm
    // hit under the exact `reduction_context` identity).
    materialized
}

/// Reduce a per-member surface value (a settled [`SemanticNodeId`]) to
/// its published [`crate::project_semantic_dispatch::raise::MaterializedTypeExpr`]
/// form WITHOUT round-tripping through
/// [`crate::project_semantic_dispatch::ProjectSemanticDispatch::shallow_lower_type_expr`].
///
/// # Why
///
/// The TypeExpr-start entry
/// [`materialize_component_meta_type_expr_until_stable_full`] is the
/// legitimate entry for callers whose inputs are parser-produced
/// `TypeExpr` annotations (e.g. `reduce_published_field_types`'s slot /
/// model bindings). It calls the context-aware TypeExpr materializer.
/// Per-member projectors that already hold a
/// `SurfaceMember.value: SemanticNodeId` (a settled graph node) should
/// NOT pay the OXC lowerer round-trip: the lower step would lower the
/// already-raised `TypeExpr` back to a graph node we already had.
///
/// # Shape
///
/// Graph-native: walks the reachable subgraph of `member_value` via
/// [`crate::project_semantic_dispatch::ProjectSemanticDispatch::raise_and_reduce`]
/// (which dispatches per shape through `execute_cooperative`). NOT
/// path-precise: the iterative reducer pushes children before reducing
/// parents, so the full subgraph of `member_value` is visited.
/// Path-precision is provided at a higher layer by the per-member
/// `MemberShapeCacheDb` amortising sibling reuse, not by the reducer
/// itself.
///
/// # Returns
///
/// A [`crate::project_semantic_dispatch::raise::MaterializedTypeExpr`]
/// carrying the reduced node id, the raised `TypeExpr`, and the
/// accumulated `DepSignature` — the same envelope
/// [`materialize_component_meta_type_expr_until_stable_full`] returns,
/// so the per-member cache can hold the identical shape.
///
/// # Dep facts
///
/// Dual-emits the accumulated `dep_signature` into the active fact
/// tracer + `DISPATCH_DEP_SIGNATURE_ACCUMULATOR`, mirroring the
/// TypeExpr entry's contract.
/// Context-explicit per-member graph-native reducer entry
/// (codex-hybrid spec).
///
/// The caller supplies the publication
/// [`crate::semantic_query::ProjectionReductionContext`] that flows
/// through every operator dispatch and through the iterative reducer's
/// demand-traversal selection. `Published(Navigate)` is the per-prop
/// publication boundary that keeps the demanded terminal shallow;
/// `Published(Expanded)` is the whole-surface mode. The pre-AX
/// `reduce_member_value_graph_native(_, _, _, ProjectionMode)` entry
/// was retired — there is one behaviour path and one entry point.
pub(crate) fn reduce_member_value_graph_native_with_context(
    ctx: &dyn crate::resolver_core::ResolverContext,
    _scope_canonical_id: &str,
    member_value: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);

    // Drive the graph-native reducer DIRECTLY on `member_value` under
    // the caller's `context`. NO `shallow_lower_type_expr` round-trip.
    // `raise_and_reduce_with_context` internally:
    //   1. `reduce_graph_node_iterative(member_value, context, …)` —
    //      top-down demand-driven worklist that pushes ONLY the
    //      children the demand context requires (per the codex-hybrid
    //      traversal rules).
    //   2. `raise_node_to_type_expr(reduced)` — single terminal raise.
    //   3. Returns `MaterializedTypeExpr { node_id, type_expr,
    //      dep_signature }`.
    let materialized = dispatch.raise_and_reduce_with_context(member_value, context);

    // Dual-emit dispatch facts into BOTH downstream channels:
    // (1) the legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` drained at
    // `compute_component_meta_state_inner` into `state.fact_versions`,
    // and (2) the `ACTIVE_TRACERS` stack captured by the outer
    // `with_fact_tracer` scope. The bridge helper drops route- and
    // project-generation entries (only `WholeHash` survives the
    // conversion); the dropped entries are R20-only signals with no
    // `FactVersionRef` equivalent. Mirrors the TypeExpr entry's
    // contract exactly.
    emit_dispatch_dep_signature_facts(ctx, &materialized.dep_signature);

    materialized
}

pub(crate) fn type_expr_has_package_backed_object_like_root(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    type_expr_has_package_backed_object_like_root_with_fence(expr, scope_canonical_id, query_engine)
        .0
}

/// Variant of [`type_expr_has_package_backed_object_like_root`] that
/// also returns the observed declaration-scope dependency fence.
///
/// Used by the projector's gate-short-circuit admit
/// paths to thread the package-backed gate's cross-file deps into the
/// cache entry's `fact_dep_signature`. Records the
/// `declaration_scope` (and, when distinct, the prepared
/// `target_scope`) so a content edit to the declaring file
/// invalidates the cached gate-shortcut entry.
///
/// The second tuple element is `Option<DepSignature>`:
///
///   * `Some(fence)` — the fence is rooted on `authoritative_current_content_hash`
///     observations for every contributing canonical (consistent with
///     `resolve_type_declaration` / `named_decl_body`'s own internal
///     hash observation). Callers may admit a cache entry rooted on
///     this fence.
///   * `None` — the gate observed an unavailable
///     `authoritative_current_content_hash` for at least one
///     contributing canonical (e.g. evicted / tombstoned mid-gate).
///     Callers MUST refuse shared admission of any cache entry whose
///     validity depends on this gate verdict: rooting an admit on a
///     stand-in hash (the pre-H2 `shallow_file_state.whole_hash`
///     `unwrap_or_default()` was a `WholeHash(0)` sentinel that does
///     not validate the actual file state — see codex P1) would
///     produce a future warm hit that returns the gate's stale verdict
///     against a fresh whole-hash with no invalidation rail.
///
/// The verdict `bool` is the gate's predicate answer; it is returned
/// regardless of whether the fence is available — non-admitting
/// callers still steer their control flow on it.
pub(crate) fn type_expr_has_package_backed_object_like_root_with_fence(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> (bool, Option<crate::semantic_query::DepSignature>) {
    use std::sync::Arc;

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

    // Empty fence carries no cross-file deps but is still safe to
    // admit on: the caller's `engine_fact_signature_for_materialize_memo`
    // self-roots on `scope_canonical_id` alone, and there are no
    // additional canonicals to root.
    let empty_fence: crate::semantic_query::DepSignature = Arc::from(Vec::new());

    let Some(root_name) = root_name(expr) else {
        return (false, Some(empty_fence));
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
        return (false, Some(empty_fence));
    }

    // H2 fence collection: record each contributing canonical via
    // `authoritative_current_content_hash` — the SAME oracle
    // `resolve_type_declaration`'s `get_or_compute` observes
    // (registry_decl.rs `observed_keyed_hash`) and `named_decl_body`
    // routes through. Using a different oracle (the pre-H2
    // `shallow_file_state(canonical).whole_hash` read) opened a race
    // window: a dependency edit between the gate verdict and the
    // fence read could admit the stale verdict against a fresh
    // whole-hash; immediate revalidation then succeeded and future
    // reads reused stale shape data.
    //
    // If any contributing canonical's authoritative current-content
    // hash is UNAVAILABLE (`None` — canonical was evicted /
    // tombstoned / cannot be authoritatively resolved without
    // permissive fallback), refuse the fence by returning `None`.
    // The caller MUST then refuse cache admission for the verdict;
    // rooting on a stand-in hash (`WholeHash(0)` sentinel) does NOT
    // validate the actual file state.
    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut refused = false;
    {
        let mut push_scope_fence = |canonical: &str| {
            if refused {
                return;
            }
            // Skip the keyed scope itself — the cache entry already self-roots
            // on it via `engine_fact_signature_for_materialize_memo`'s
            // `FileWholeHash` for `scope_canonical_id`.
            if canonical == scope_canonical_id || canonical.is_empty() {
                return;
            }
            match query_engine
                .ctx
                .authoritative_current_content_hash(canonical)
            {
                Some(whole_hash) => {
                    fence.push((
                        Arc::<str>::from(canonical),
                        crate::semantic_query::DepVersion::WholeHash(whole_hash),
                    ));
                }
                None => {
                    refused = true;
                }
            }
        };
        push_scope_fence(declaration_scope.as_str());
    }

    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        if refused {
            return (true, None);
        }
        let fence_sig: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
        return (true, Some(fence_sig));
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    // Record the prepared-target scope too when it diverges from the
    // declaration scope (e.g. cross-file `export type X = Y` re-export
    // chain). The prepared resolver may walk arbitrarily far; the
    // terminal scope is the one whose `named_decl_body` we read below.
    {
        let mut push_scope_fence = |canonical: &str| {
            if refused {
                return;
            }
            if canonical == scope_canonical_id || canonical.is_empty() {
                return;
            }
            match query_engine
                .ctx
                .authoritative_current_content_hash(canonical)
            {
                Some(whole_hash) => {
                    fence.push((
                        Arc::<str>::from(canonical),
                        crate::semantic_query::DepVersion::WholeHash(whole_hash),
                    ));
                }
                None => {
                    refused = true;
                }
            }
        };
        push_scope_fence(target_scope.as_str());
    }
    let verdict = query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(&body)
        });
    if refused {
        return (verdict, None);
    }
    let fence_sig: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
    (verdict, Some(fence_sig))
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
