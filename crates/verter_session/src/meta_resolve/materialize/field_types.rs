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
//! `reduce_field_type_expr_with_mode`) is the sole post-projection
//! authority for finalising published field types.

use crate::instant::Instant;

use super::super::dep_signature::emit_dispatch_dep_signature_facts;
use super::super::registry_materialize::preserve_package_backed_symbolic_refs_node;

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The whole-expression field-type MATERIALISER's output-sink
    /// capability. The materialiser here holds this to reduce-then-raise a
    /// graph node into a sealed output carrier and unwrap it. Its
    /// constructor is visible ONLY within
    /// `crate::meta_resolve::materialize::field_types` — NOT the whole
    /// `meta_resolve` subtree — so the Kind-B bridge sibling
    /// `meta_resolve::dispatch_helpers` cannot mint it (planted
    /// `MetaResolveFieldTypesOutputCap::new` there ⇒ `E0624`).
    pub(crate) struct MetaResolveFieldTypesOutputCap;
    mint: pub(in crate::meta_resolve::materialize::field_types)
}

/// Non-output capability authorising construction of a registry member-VALUE-NODE
/// [`crate::component_meta_caches::ShapeCacheKey`] from a RAW `SemanticNodeId`.
///
/// It is deliberately NOT an [`crate::project_semantic_dispatch::output_materialization::OutputProjector`]:
/// it carries no materialisation/unseal power — it cannot turn a node into a
/// `TypeExpr`. Its sole purpose is to gate the member-value-node key constructor
/// the same way [`crate::component_meta_caches::ShapeCacheKey::surface_member_value_whole_with_context`]
/// gates that key behind a POLICY-ADMITTED `AdmittedPublishedMember` token, so an
/// arbitrary `SemanticNodeId` cannot spread into the sealed member-shape subject.
/// The registry member-surface stabiliser holds the producing node (the first-pass
/// `MaterializeStructureDb` output) directly rather than a re-derived
/// `AdmittedPublishedMember`, so it needs this distinct, narrower authorisation.
///
/// Constructor visible ONLY within `crate::meta_resolve::materialize::field_types`
/// (a planted `RegistryMemberShapeKeyCap::new` outside this leaf is `E0624`), so
/// no other module can route a raw node through the sealed key.
pub(crate) struct RegistryMemberShapeKeyCap {
    _seal: (),
}

impl RegistryMemberShapeKeyCap {
    pub(in crate::meta_resolve::materialize::field_types) fn new() -> Self {
        Self { _seal: () }
    }
}

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
/// The `ShapeCacheDb` peek/publish slot MUST be keyed by this
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
/// slot-key poisoning. This helper is the single source for both the
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

/// Node-domain mirror of [`type_expr_root_is_published_operator`] applied to a
/// node's RAISED root term: is `node`'s root a published operator
/// (`Ref`/`Mapped`-with-value/`KeyOf`/`IndexedAccess`/`Conditional`/`TypeOf`)?
///
/// Read off the producing node so the second-pass reduction context is decided
/// without raising the first-pass surface to a `TypeExpr` and re-classifying it.
/// Arm-for-arm with the `TypeExpr` predicate:
///   - `Alias(inner)` ⇒ recurse — the node-domain identity hop (the `TypeExpr`
///     predicate's `Parenthesized` peel); the raise strips it.
///   - `DeclRef` / `InstantiationRef` / `BareRef` ⇒ true — they raise to
///     `TypeExpr::Ref` (a surviving declared surface root).
///   - `Mapped { value }` ⇒ true UNLESS the `value` raises to an unmaterialised
///     miss placeholder (the node form of the `TypeExpr` predicate's
///     `value == Unknown { raw == "semanticMiss" }` carrier check). The node
///     sentinel recogniser is the established miss-placeholder authority; it is
///     marginally broader than the single `"semanticMiss"` spelling, which only
///     differs for a `Mapped` value raising to one of the OTHER sentinel spellings
///     (e.g. an object-surface placeholder) — a shape the parity test pins.
///   - `KeyOf` / `IndexedAccess` / `Conditional` / `TypeOf` ⇒ true.
///   - everything else (`Object`/`Union`/`Intersection`/primitives/… and
///     `ImportType`/`RawFallback`, which raise to non-`Ref` non-operator terms) ⇒
///     false.
fn node_root_is_published_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::SemanticNodeData;

    fn walk(
        dispatch: &ProjectSemanticDispatch<'_>,
        node: crate::semantic_query::SemanticNodeId,
        depth: u32,
    ) -> bool {
        const MAX_DEPTH: u32 = 32;
        if depth >= MAX_DEPTH {
            return false;
        }
        let Some(data) = dispatch.graph().node_data(node) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => {
                let inner = *inner;
                drop(data);
                walk(dispatch, inner, depth + 1)
            }
            SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::BareRef(_) => true,
            SemanticNodeData::Mapped { mapper, .. } => {
                // The mapped VALUE expression (`{ [K in S]: <value> }`) lives on
                // the mapper key. The `TypeExpr` predicate keeps a `Mapped` whose
                // value is the miss placeholder as a carrier (false) and publishes
                // one with an author-visible value (true).
                let value = mapper.value_expr;
                drop(data);
                !node_root_is_unmaterialized_sentinel_with_dispatch(dispatch, value)
            }
            SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::TypeOf(_) => true,
            _ => false,
        }
    }

    let dispatch = ProjectSemanticDispatch::new(ctx);
    walk(&dispatch, node, 0)
}

/// Node-domain mirror of [`type_expr_materialize_reduction_context`]: the EXACT
/// [`crate::semantic_query::ProjectionReductionContext`] the second stabilisation
/// pass reduces a first-pass surface NODE under, given the caller's `mode`.
///
/// `Navigate` + a published-operator root ⇒ `Published(Navigate)` (the explicit
/// narrowing IS consumer demand); every other `Navigate` reduces under
/// `StructuralTransit(Navigate)`; other modes reduce under `Published(mode)`.
/// Mirrors the `TypeExpr`-start materialiser so the registry member-surface
/// stabiliser reduces under the SAME context `_until_stable_full` used to.
pub(crate) fn node_materialize_reduction_context(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate)
        && node_root_is_published_operator(ctx, node)
    {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    } else {
        type_expr_materializer_context(mode)
    }
}

/// The node-first second-pass result: the registry member surface AFTER the
/// graph-native stabilisation reduce, already no-poison-selected against the
/// first pass.
///
/// - [`Self::First`] — the stabilisation INTRODUCED an unmaterialised miss the
///   first-pass surface did not carry (an owner-scope reduction miss on a
///   foreign-file carrier is "no reduction available here", not "unknown"), so
///   the FIRST-pass surface node wins. The caller materialises `node` once.
/// - [`Self::Stable`] — the stabilised carrier wins (the common path). The caller
///   unwraps `carrier` once.
///
/// Both variants carry the chosen NODE + its [`RaisedShapeFacts`] so the candidate
/// sibling raises that node ONCE at a registered terminal sink and reads downstream
/// facts (object-surface, miss) off the SAME node it publishes, never by re-deciding
/// on the materialised `TypeExpr`. (The reduce's `MaterializedOutputTypeExpr` carrier
/// is consumed inside the stabiliser — its `dep_signature` is emitted there and its
/// node is the `Stable.node`; re-raising that node at the sink reproduces the
/// carrier's value byte-for-byte without a non-sink `into_type_expr`.)
pub(crate) enum RegistryMemberStabilizedValue {
    First {
        node: crate::semantic_query::SemanticNodeId,
    },
    Stable {
        node: crate::semantic_query::SemanticNodeId,
    },
}

/// Second stabilisation pass for a registry member surface, node-first.
///
/// `first_node` is the FIRST pass (`MaterializeStructureDb`) surface node. This
/// reduces it ONCE through the graph-native
/// [`reduce_member_value_graph_native_with_context`] reducer behind the host-owned
/// [`crate::component_meta_caches::ShapeCacheDb`] member-VALUE-node slot (keyed by
/// [`crate::component_meta_caches::ShapeCacheKey::registry_member_value_node_whole_with_context`]
/// under the EXACT [`node_materialize_reduction_context`]), reproducing
/// [`materialize_component_meta_type_expr_until_stable_full`]'s reduction context +
/// cache-admission rails — but WITHOUT lowering the first-pass surface back to a
/// `TypeExpr` and re-reducing it (the regression this redo removes). The reducer
/// reuses the settled `first_node` directly.
///
/// No-poison (tri-state): the stabilisation result is kept UNLESS it introduced an
/// unmaterialised miss the first pass did not carry — decided on the node-domain
/// `!RaisedShapeFacts.materialized` fact off each node, NEVER on the materialised
/// `TypeExpr`. `None` facts (raise could not compute) are NOT collapsed to "no
/// miss": the first pass wins ONLY when the stabilised root is CONFIDENTLY a miss
/// AND the first-pass root is CONFIDENTLY miss-free.
pub(crate) fn stabilize_registry_member_surface_node_with_shape_cache(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical_id: &str,
    first_node: crate::semantic_query::SemanticNodeId,
    mode: crate::semantic_query::ProjectionMode,
) -> RegistryMemberStabilizedValue {
    use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use std::sync::Arc;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let first_facts = node_raised_shape_facts_with_dispatch(&dispatch, first_node);

    // The reduction context the second pass runs under — keyed onto the slot so a
    // stored value (reduced under this context) is only served to a consumer that
    // reduces under the SAME context.
    let reduction_context = node_materialize_reduction_context(ctx, first_node, mode);
    let cap = RegistryMemberShapeKeyCap::new();
    let key =
        crate::component_meta_caches::ShapeCacheKey::registry_member_value_node_whole_with_context(
            Arc::<str>::from(scope_canonical_id),
            &cap,
            first_node,
            reduction_context,
        );

    // Stabilise: peek the ShapeCacheDb member-node slot, else cold-reduce the
    // first-pass node once through the graph-native reducer and admit.
    let stabilized = {
        let cache = ctx.project_type_store().shape_cache_db();
        if let Some(cached) = cache.peek(&key, ctx) {
            emit_dispatch_dep_signature_facts(ctx, cached.dep_signature());
            cached
        } else {
            let materialized = reduce_member_value_graph_native_with_context(
                ctx,
                scope_canonical_id,
                first_node,
                reduction_context,
            );
            // A GENUINE-partial reduce (budget-tripped contributing read) must NOT
            // warm the shared slot — refused admission, the no-poison invariant.
            if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                materialized.result_is_partial(),
            ) {
                materialized
            } else {
                let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
                let materialized_for_closure = materialized.clone();
                let admitted = cache.get_or_compute(&key, ctx, move || {
                    let scope_obs = observed_scope?;
                    let parse_fact = scope_obs.syntactic_export_set.clone()?;
                    match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                        &scope_obs,
                        parse_fact,
                        materialized_for_closure.dep_signature(),
                    ) {
                        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                            Some((materialized_for_closure, sig.facts))
                        }
                        crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
                    }
                });
                admitted.unwrap_or(materialized)
            }
        }
    };

    // The stabilised NODE is the reduced carrier's node; a reduce that settled no
    // node falls back to the first-pass node (a degenerate reduce ⇒ publish the
    // first pass). The carrier itself is dropped here — its dep_signature was
    // emitted above, and the candidate sibling re-raises this node at a registered
    // sink to reproduce its value.
    let stable_node = stabilized.node_id().unwrap_or(first_node);
    let stable_facts = node_raised_shape_facts_with_dispatch(&dispatch, stable_node);

    // No-poison (tri-state): keep the FIRST-pass surface only when the stabilised
    // root is CONFIDENTLY a miss AND the first-pass root is CONFIDENTLY miss-free.
    let stable_has_miss = stable_facts.map(|f| !f.materialized);
    let first_has_miss = first_facts.map(|f| !f.materialized);
    if stable_has_miss == Some(true) && first_has_miss == Some(false) {
        RegistryMemberStabilizedValue::First { node: first_node }
    } else {
        RegistryMemberStabilizedValue::Stable { node: stable_node }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_type_expr::TypeExpr {
    // `into_type_expr` is an inherent capability-gated accessor on the
    // carrier, so the `OutputProjector` trait need not be imported here.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let full = materialize_component_meta_type_expr_until_stable_full(
        expr,
        scope_canonical_id,
        mode,
        query_engine,
    );
    // Publication-shell read: unwrap the sealed payload via the meta-resolve
    // output capability (this shell is a true output sink that publishes the
    // bare `TypeExpr`).
    let dispatch = ProjectSemanticDispatch::new(query_engine.ctx());
    let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
    full.into_type_expr(&cap)
}

/// Materialize a `TypeExpr` and return both the result and the
/// producing `SemanticNodeId` + accumulated dep_signature
/// ([`MaterializedOutputTypeExpr`]). Sidecar-capture call sites read
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
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    use crate::project_semantic_dispatch::output_materialization::{
        wrap_output_type_expr, OutputProjector,
    };
    use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
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

    // The host-owned `ShapeCacheDb` is THE materialiser cache.
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

    // Classify the shape-cache key ONCE per materialization pass and reuse
    // the SAME `Option<ShapeCacheKey>` for both the peek (below) and the
    // admit (further down). The classifier runs the depth-safe synthetic-
    // carrier walker (`NonSyntheticTypeExpr::new`) per build, so building
    // it separately for the peek and the admit double-walked every
    // carrier-free expression. `None` (a composite NESTING a synthetic
    // carrier — no sound content-free key) still bypasses BOTH the peek
    // and the admit; the value is computed and returned either way.
    let cache_key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
        std::sync::Arc::<str>::from(scope_canonical_id),
        std::sync::Arc::new(expr.clone()),
        reduction_context,
    );

    // Peek the universal ShapeCacheDb (TypeExpr subject,
    // whole-subject demand under the exact reduction context).
    {
        // Loop-5 instrumentation — bump peek for every host-memo
        // read attempt; bump hit only on the cached return path.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PEEKS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let ctx = query_engine.ctx();
        // A composite expression that NESTS a synthetic carrier has no
        // sound content-free cache key — bypass the cache (skip peek; the
        // admit path below is likewise skipped) and fall through to the
        // full cold compute. A bare carrier / carrier-free expression
        // yields a key normally.
        if let Some(cache_key) = &cache_key {
            let host_db = ctx.project_type_store().shape_cache_db();
            if let Some(cached) = host_db.peek(cache_key, ctx) {
                crate::loop5_instrumentation::MATERIALIZE_MEMO_HITS
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return cached;
            }
        }
    }

    // Snapshot the request-scoped materialization suppress sticky BEFORE
    // this compute lowers/reduces. A `typeof <unresolved import>` is a
    // genuine `MissingDependency` partial whose only invalidation rail is the
    // owner's `ImportRoute` derived fact — a rail the build-layer fence cannot
    // carry and the consuming `raise_and_reduce`'s eager TypeOf lowering path
    // resolves through a partial-dropping `execute_type_node`. The producer
    // (`build_typeof`) marks THIS request's materialization suppress sticky for
    // such a miss, so a sticky that transitions from unset→set DURING this
    // compute is the precise signal that this materialisation observed a
    // MissingDependency. Such a result MUST be `ReturnOnly` (refused warm
    // admission) so the next request after the dependency appears recomputes
    // cold and recovers. (Per the architecture ruling: an unrootable
    // MissingDependency is `ReturnOnly`.)
    let suppress_sticky_before = crate::request_context::current_materialization_cache_suppress();

    // Step 1.5 thin dispatch wrapper. Build NodeScopeId for the file
    // scope, then lower → raise_and_reduce in the caller's mode.
    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    // Capture the scope-shadowing context once for the materialize → lower
    // pipeline from the per-scope memo, so the dispatch fast-path observes the
    // same shadow set the route-extraction path uses. The `&mut` borrow ends
    // here (the accessor returns an owned `Arc<ScopeShadowing>`), so the shared
    // `ctx` borrow opened just below — held through dispatch lowering — is
    // unaffected. The memo's `from_scope_payload` reuses the SAME just-cached
    // scope payload captured above, so its shadow set is membership-identical to
    // an inline `from_scope_payload(scope_payload)` build (same payload in, same
    // set out).
    let shadowing = query_engine.scope_shadowing_for_scope(scope_canonical_id);
    let ctx = query_engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Mint the field-types materialiser output capability (constructor
    // visible only within `crate::meta_resolve::materialize::field_types`):
    // this materialiser is a true publication sink — it reduce-then-raises
    // into a sealed carrier and unwraps the sealed payload via the capability.
    let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
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
        return MaterializedOutputTypeExpr::from_parts(
            None,
            wrap_output_type_expr(&cap, expr.clone()),
            Arc::from(Vec::new()),
            false,
        );
    };
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash: observed_scope_whole_hash,
        local_scope: None,
    };
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    let _us_trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
    if _us_trace {
        eprintln!(
            "[US_LOWER_START] scope={} mode={:?}",
            scope_canonical_id, mode
        );
    }
    // `reduction_context` was computed at function entry so the
    // ShapeCacheDb peek/publish key shares one identity with the value.
    let _us_lower_t0 = Instant::now();
    let lowered = dispatch.shallow_lower_type_expr_with_context(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        shadowing.as_ref(),
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
    let materialized = cap.materialize_reduced_output_type_expr(lowered, reduction_context);
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
    emit_dispatch_dep_signature_facts(ctx, materialized.dep_signature());

    // `materialized` is the sealed reduce-then-raise carrier the boundary
    // produced; thread it through verbatim. Its `type_expr` payload is read
    // below only through the capability-gated accessor.

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
    // Additional gate: `result_is_partial=true` on the freshly materialized
    // value means a downstream `dispatch.execute_read(...)` exhausted the
    // projection-op budget (or returned another fatal `QueryError` / a
    // same-path recursion / a walker fatal). The PARTIAL outcome must NOT
    // warm the shared `ShapeCacheDb` slot — admitting it would poison
    // subsequent identical-key lookups against the same scope+expr+mode
    // triple. A benign non-cacheable read (ReturnOnly / overflow /
    // unrootable self-root) does NOT set `result_is_partial`, so a
    // complete-but-non-cacheable materialisation still warms the shape
    // cache here. The freshly-computed value is always returned to the
    // caller; only the shared-cache admission is refused for a partial.
    //
    // Cross-batch budget determinism is enforced by COMPLETENESS-based macro
    // admission, NOT by charging warm cache hits: a budget-exhausted macro
    // surface carries `ResultCompleteness::Partial` and is refused admission at
    // EVERY shared cache boundary — the `vue_surface_store` DTO boundary AND the
    // `ComponentMetaResultDb` / resolved-meta final-result caches. A repeat
    // batch therefore re-resolves the partial owner cold (no laundered warm
    // replay through the surface store), so its per-result completeness is
    // re-observed even though its per-arm `Instantiate` memos are warm.
    //
    // A MissingDependency observed DURING this compute (the suppress sticky
    // transitioned unset→set) makes this materialisation a `ReturnOnly`
    // partial: refuse the shared-cache admission so a stale miss cannot be
    // served after the dependency appears. A request whose sticky was ALREADY
    // set on entry (an unrelated earlier miss) does not taint this entry.
    let observed_missing_dependency =
        !suppress_sticky_before && crate::request_context::current_materialization_cache_suppress();
    // A `typeof <X>` materialisation whose RESULT ROOT is the unmaterialised /
    // semanticMiss sentinel is a `MissingDependency` (an `import X from
    // './missing'` whose specifier does not yet resolve): its only invalidation
    // rail is the owner's `ImportRoute` derived fact — a rail the build-layer
    // fence cannot carry — so admitting the miss-rooted value into the shared
    // `ShapeCacheDb` would stale-serve it after the dependency appears. (The
    // request's materialization suppress sticky may have been set by an EARLIER
    // `build_typeof` sub-read in the SAME request, so the unset→set transition
    // check alone misses this.) The check is SCOPED to a `TypeOf`-rooted expr:
    // an unresolved value-reference is the MissingDependency class; a
    // miss-rooted `Pick`/`Omit`/`Ref` materialisation is a DIFFERENT class
    // (genuine unresolved symbol / closed-source path-precise miss) the
    // surrounding gates already handle, and refusing those here would regress
    // the closed-source path-precise admission. Refuse only the typeof miss so
    // the next request recomputes cold and recovers. The value still flows.
    let typeof_result_root_is_miss = matches!(expr, verter_type_expr::TypeExpr::TypeOf(_))
        && materialized.node_id().is_some_and(|node| {
            crate::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch(
                &dispatch, node,
            )
        });
    if !materialized.result_is_partial()
        && !observed_missing_dependency
        && !typeof_result_root_is_miss
    {
        if let Some(captured_scope_observation) = observed_scope {
            // Loop-5 instrumentation — count every publish attempt. The
            // get_or_compute path is a no-op on a concurrent winner but
            // we count the attempt because the bench is single-threaded.
            crate::loop5_instrumentation::MATERIALIZE_MEMO_PUBLISHES
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // A composite expression that NESTS a synthetic carrier has
            // no sound content-free cache key — skip the cache admit
            // entirely (the freshly-computed `materialized` value still
            // flows to the caller below). A bare carrier / carrier-free
            // expression yields a key normally. The SAME `cache_key` the
            // peek classified at function entry is reused here — the
            // classifier (and its synthetic-carrier walk) runs once per
            // materialization pass, not once per peek and once per admit.
            if let Some(cache_key) = &cache_key {
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
                let _ = host_db.get_or_compute(cache_key, ctx, move || {
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
            match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                &captured_scope_observation,
                observed_scope_syntactic_export_set,
                captured_value.dep_signature(),
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    Some((captured_value, sig.facts))
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
            }
        });
            }
        }
    }

    // No request-local write-through. The host-owned
    // `ShapeCacheDb` get_or_compute above is the SOLE materialiser
    // cache; the same request's later reduce calls re-peek it (a warm
    // hit under the exact `reduction_context` identity).
    materialized
}

/// Reduce a per-member surface value (a settled [`SemanticNodeId`]) to
/// its published [`crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr`]
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
/// [`crate::project_semantic_dispatch::ProjectSemanticDispatch::raise_and_reduce_with_context`]
/// (which dispatches per shape through `execute_cooperative`). NOT
/// path-precise: the iterative reducer pushes children before reducing
/// parents, so the full subgraph of `member_value` is visited.
/// Path-precision is provided at a higher layer by the per-member slot
/// of [`crate::component_meta_caches::ShapeCacheDb`] (indexed by
/// [`crate::component_meta_caches::ShapeSubject::MemberValueNode`] via
/// `ShapeCacheKey::surface_member_value_whole_with_context`), which
/// amortises sibling reuse, not by the reducer itself.
///
/// # Returns
///
/// A [`crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr`]
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
/// (demand-driven reducer spec).
///
/// The caller supplies the publication
/// [`crate::semantic_query::ProjectionReductionContext`] that flows
/// through every operator dispatch and through the iterative reducer's
/// demand-traversal selection. `Published(Navigate)` is the per-prop
/// publication boundary that keeps the demanded terminal shallow;
/// `Published(Expanded)` is the whole-surface mode. The earlier
/// `reduce_member_value_graph_native(_, _, _, ProjectionMode)` entry
/// is gone — there is one behaviour path and one entry point.
pub(crate) fn reduce_member_value_graph_native_with_context(
    ctx: &dyn crate::resolver_core::ResolverContext,
    _scope_canonical_id: &str,
    member_value: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    use crate::project_semantic_dispatch::output_materialization::OutputProjector;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Publication sink: reduce-then-raise into the sealed carrier via the
    // meta-resolve output capability.
    let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);

    // Drive the graph-native reducer DIRECTLY on `member_value` under
    // the caller's `context`. NO `shallow_lower_type_expr` round-trip.
    // `raise_and_reduce_with_context` internally:
    //   1. `reduce_graph_node_iterative(member_value, context, …)` —
    //      top-down demand-driven worklist that pushes ONLY the
    //      children the demand context requires (per the demand-driven
    //      reducer traversal rules).
    //   2. `raise_node_to_type_expr(reduced)` — single terminal raise.
    //   3. Returns the sealed `MaterializedOutputTypeExpr` (node_id +
    //      sealed type_expr payload + dep_signature + result_is_partial).
    let materialized = cap.materialize_reduced_output_type_expr(member_value, context);

    // Dual-emit dispatch facts into BOTH downstream channels:
    // (1) the legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` drained at
    // `compute_component_meta_state_inner` into `state.fact_versions`,
    // and (2) the `ACTIVE_TRACERS` stack captured by the outer
    // `with_fact_tracer` scope. The bridge helper drops route- and
    // project-generation entries (only `WholeHash` survives the
    // conversion); the dropped entries are R20-only signals with no
    // `FactVersionRef` equivalent. Mirrors the TypeExpr entry's
    // contract exactly.
    emit_dispatch_dep_signature_facts(ctx, materialized.dep_signature());

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
///     stand-in hash (a `shallow_file_state.whole_hash`
///     `unwrap_or_default()` `WholeHash(0)` sentinel does
///     not validate the actual file state) would
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

    // Empty fence carries no cross-file deps but is still safe to
    // admit on: the caller's `engine_fact_signature_for_materialize_memo`
    // self-roots on `scope_canonical_id` alone, and there are no
    // additional canonicals to root.
    let empty_fence: crate::semantic_query::DepSignature = Arc::from(Vec::new());

    let Some(root_identity) = type_expr_root_identity(query_engine, scope_canonical_id, expr)
    else {
        return (false, Some(empty_fence));
    };
    package_backed_object_like_root_identity_with_fence(
        query_engine,
        scope_canonical_id,
        &root_identity,
    )
}

/// Extract the package-backed gate's ROOT declaration IDENTITY from a `TypeExpr` —
/// the `TypeExpr` front of the SHARED root-identity tail
/// ([`package_backed_object_like_root_identity_with_fence`]). Resolver-aware: a
/// `Pick`/`Omit` SOURCE-root descent (into `args[0]`) fires ONLY when `name`
/// actually resolves to the builtin utility (no userland `type Pick` shadow), so
/// it agrees with the node front's `base.canonical_id == "__builtin__"` check.
/// `Alias`/`Parenthesized` peels and `IndexedAccess` descends to the object root,
/// exactly as the node front does.
fn type_expr_root_identity(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<crate::semantic_query::DeclIdentity> {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            type_expr_root_identity(query_engine, scope_canonical_id, inner)
        }
        TypeExpr::IndexedAccess { object, .. } => {
            type_expr_root_identity(query_engine, scope_canonical_id, object)
        }
        // `Pick<Source, K>` / `Omit<Source, K>` — descend to the SOURCE root, but
        // ONLY when `name` is the BUILTIN utility (a userland `type Pick` shadow is
        // its OWN root, never a source descent).
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 2
            && is_builtin_pick_or_omit(query_engine, scope_canonical_id, name) =>
        {
            type_expr_root_identity(query_engine, scope_canonical_id, &type_arguments[0])
        }
        TypeExpr::Ref { name, .. } => Some(resolve_ref_to_root_identity(
            query_engine,
            scope_canonical_id,
            name,
        )),
        _ => None,
    }
}

/// Whether `name` is the UNSHADOWED builtin `Pick`/`Omit` utility at
/// `scope_canonical_id` — the SAME builtin/shadow decision dispatch's
/// [`resolve_bare_ref_head`](crate::project_semantic_dispatch) makes before
/// minting a `__builtin__::Pick`/`Omit` carrier: `name` is a recognised
/// object-filter utility ([`BuiltinUtility::from_name`]) AND no userland
/// declaration shadows it.
///
/// Shadowing is decided by the SINGLE-SOURCE-OF-TRUTH
/// [`ScopeShadowing::is_shadowing_lib`](crate::resolver_core::scope_shadowing::ScopeShadowing::is_shadowing_lib),
/// the same authority the dispatch path consults — it folds the owner scope's
/// local type names, script-setup type bindings, AND RESOLVED import bindings (an
/// import whose module resolves to a canonical id, even when that module does not
/// actually export the name). So a local `type Pick`, a script-setup
/// `generic="Pick"`, OR an imported `Pick` whose module resolves ALL shadow the
/// builtin and resolve to their OWN root, never a source-descent into the
/// utility's argument.
///
/// This is the `TypeExpr`-front mirror of the node front's
/// `InstantiationRef.base.canonical_id == "__builtin__"` check — which reads the
/// SAME `is_shadowing_lib`-gated identity dispatch already minted — so the two
/// fronts agree by construction rather than via a parallel heuristic. A
/// `resolve_type_declaration(...).kind == Unknown` check would MISCLASSIFY this:
/// it cannot tell "imported, module resolves" (kind == Unknown, yet shadowing)
/// apart from "ambient builtin" (kind == Unknown, NOT shadowing) — which is why
/// the gate reads the shadow set directly.
///
/// [debt] Shared-`ScopeShadowing` limitation (resolver_core), shared with the
/// dispatch path: the shadow set is built from `import_bindings`, which omits an
/// UNRESOLVED-SPECIFIER import (`import { Pick } from "./missing"` whose module
/// resolves to no canonical id — `prepared_decl` records a binding only when the
/// module resolves). Such an import escapes the shadow set, so a builtin-colliding
/// name imported from an unresolvable module is classified as the builtin here AND
/// in dispatch's `resolve_bare_ref_head` (the two stay in agreement). Closing it
/// is a shared-owner follow-up: carry a lexical import-name set in the scope
/// payload (from `import_targets` / `import_locals`, independent of resolution)
/// and consult it in `ScopeShadowing`.
fn is_builtin_pick_or_omit(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    name: &str,
) -> bool {
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;

    if !matches!(
        BuiltinUtility::from_name(name),
        Some(BuiltinUtility::Pick | BuiltinUtility::Omit)
    ) {
        return false;
    }
    // The per-scope shadow set is built ONCE (memoized on the engine beside the
    // scope payload) and reused across every Pick/Omit probe, so this gate is
    // O(1) per published field — a hash-set membership check — rather than
    // folding a fresh shadow set (`FxHashSet` + `Arc<str>` entries) from the
    // prepared-decl bundle on each field. The cached shadow set is identical to
    // the `from_host_scope` bundle-derived one dispatch consumes: both fold the
    // scope's local type names, script-setup type bindings, and resolved import
    // bindings, so the `TypeExpr` front and the dispatch front agree by
    // construction.
    !query_engine
        .scope_shadowing_for_scope(scope_canonical_id)
        .is_shadowing_lib(name)
}

/// Resolve a bare `Ref` `name` (at `scope_canonical_id`) to its ROOT declaration
/// [`crate::semantic_query::DeclIdentity`] — the resolved declaring file +
/// declaration name + that file's whole-hash. Mirrors the cycle front's
/// `collect_root_decl_identities` identity construction so both fronts root on
/// the SAME identity.
fn resolve_ref_to_root_identity(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    name: &str,
) -> crate::semantic_query::DeclIdentity {
    use std::sync::Arc;

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let canonical_id: Arc<str> = if declaration.canonical_source.is_empty() {
        Arc::from(scope_canonical_id)
    } else {
        Arc::from(declaration.canonical_source.as_str())
    };
    let decl_name: Arc<str> = if declaration.resolved_name.is_empty() {
        Arc::from(name)
    } else {
        Arc::from(declaration.resolved_name.as_str())
    };
    let whole_hash = query_engine
        .ctx
        .shallow_file_state(canonical_id.as_ref())
        .map(|state| state.whole_hash)
        .unwrap_or_default();
    crate::semantic_query::DeclIdentity {
        canonical_id,
        whole_hash,
        decl_name,
    }
}

/// Append `canonical`'s authoritative current-content hash to `fence` (skipping
/// the keyed `scope` self-entry + empties), or set `refused` when the hash is
/// unavailable. The fence oracle is `authoritative_current_content_hash` — the
/// SAME oracle `resolve_type_declaration` / `named_decl_body` observe, so the
/// admit's fact signature invalidates on a contributing-file edit. A `None` hash
/// (evicted / tombstoned canonical) refuses shared admission: rooting on a
/// stand-in `WholeHash(0)` sentinel would not validate the file state.
fn push_decl_scope_fence(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical: &str,
    scope_canonical_id: &str,
    fence: &mut Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)>,
    refused: &mut bool,
) {
    if *refused {
        return;
    }
    // Skip the keyed scope itself — the cache entry already self-roots on it via
    // `engine_fact_signature_for_materialize_memo`'s `FileWholeHash`.
    if canonical == scope_canonical_id || canonical.is_empty() {
        return;
    }
    match ctx.authoritative_current_content_hash(canonical) {
        Some(whole_hash) => fence.push((
            std::sync::Arc::<str>::from(canonical),
            crate::semantic_query::DepVersion::WholeHash(whole_hash),
        )),
        None => *refused = true,
    }
}

/// The SHARED root-identity tail of the package-backed object-like gate: given a
/// RESOLVED root declaration `root_identity` (the declaring file + name, already
/// resolved by either front — NEVER a name re-resolved from `scope`), decide
/// whether that declaration is a package-backed object-like surface, and collect
/// the cross-file fence.
///
/// IDENTITY-PRESERVING: the kind / final-target resolution runs at
/// `root_identity.canonical_id` (the declaration's OWN file), so a node carrier
/// whose identity points at file X is never re-resolved by name from `scope` (the
/// former synthetic `TypeExpr::named(name)` bridge could hit a DIFFERENT symbol).
///
/// The second tuple element is `Option<DepSignature>`: `Some(fence)` is rooted on
/// `authoritative_current_content_hash` for every contributing canonical;
/// `None` means a contributing canonical's hash was unavailable, so callers MUST
/// refuse cache admission of the verdict.
pub(crate) fn package_backed_object_like_root_identity_with_fence(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    root_identity: &crate::semantic_query::DeclIdentity,
) -> (bool, Option<crate::semantic_query::DepSignature>) {
    use std::sync::Arc;

    let empty_fence: crate::semantic_query::DepSignature = Arc::from(Vec::new());

    let declaration_scope = root_identity.canonical_id.as_ref();
    // Issue #11 / route the package-backed classification through
    // `WorkspaceRead::is_package_backed` (NOT a path-substring check on
    // `node_modules`). The realpath-based classification correctly handles
    // pnpm-symlinks and workspace-packages-inside-node_modules.
    if !query_engine
        .ctx
        .workspace_is_package_backed(declaration_scope)
    {
        return (false, Some(empty_fence));
    }

    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut refused = false;
    push_decl_scope_fence(
        query_engine.ctx,
        declaration_scope,
        scope_canonical_id,
        &mut fence,
        &mut refused,
    );

    // Resolve the declaration KIND at its OWN file (identity-preserving — never a
    // re-resolve from `scope`).
    let declaration =
        query_engine.resolve_type_declaration(declaration_scope, root_identity.decl_name.as_ref());

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
        root_identity.decl_name.to_string()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope, declaration_name.as_str());
    // Record the prepared-target scope too when it diverges from the declaration
    // scope (e.g. cross-file `export type X = Y` re-export chain) — the terminal
    // scope is the one whose `named_decl_body` is read below.
    push_decl_scope_fence(
        query_engine.ctx,
        target_scope.as_str(),
        scope_canonical_id,
        &mut fence,
        &mut refused,
    );
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
    use crate::project_semantic_dispatch::output_materialization::OutputProjector;
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
    // Publication sink: materialize into a sealed carrier and unwrap via
    // the meta-resolve output capability.
    let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
    cap.materialize_output_type_expr(preserved_node)
        .map(|carrier| carrier.into_type_expr(&cap))
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
