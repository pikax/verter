//! Materialization core: TypeExpr stabilizer.
//!
//! Owns:
//! - the bounded fixed-point reducer
//!   (`materialize_component_meta_type_expr_until_stable` + `_full`),
//! - the shared package-backed-root identity tail
//!   (`package_backed_object_like_root_identity_with_fence`) that gates the
//!   projector's reduction decision behind the node-domain front
//!   (`node_package_backed_object_like_root_with_fence`),
//! - the test-only `MTL_CALL_COUNT` instrumentation that the
//!   eager-entry tests count off.
//!
//! The projector path
//! (`meta_resolve::projectors::reduce_published_field_types` +
//! `reduce_field_type_expr_with_mode`) is the sole post-projection
//! authority for finalising published field types.

use crate::instant::Instant;

use super::super::dep_signature::emit_dispatch_dep_signature_facts;

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
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    mode: crate::semantic_query::ProjectionMode,
) -> crate::semantic_query::ProjectionReductionContext {
    if matches!(mode, crate::semantic_query::ProjectionMode::Navigate)
        && type_expr_root_is_published_operator(ctx, scope_canonical_id, expr, mode)
    {
        crate::semantic_query::ProjectionReductionContext::published(mode)
    } else {
        type_expr_materializer_context(mode)
    }
}

fn type_expr_root_is_published_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    mode: crate::semantic_query::ProjectionMode,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            type_expr_root_is_published_operator(ctx, scope_canonical_id, inner, mode)
        }
        TypeExpr::Ref { .. } => {
            // References that survive parser lowering are declared
            // surface roots. Builtin broad mapped carriers are handled
            // by the `Mapped` arm below after dispatch lowering.
            true
        }
        TypeExpr::Mapped { .. } => {
            // Builtin broad object modifiers lower to identity mapped
            // carriers whose VALUE position is the typed miss carrier
            // (`Opaque(QueryError::Miss)`). Keep those as carriers at
            // Navigate depth; publish mapped types that carry an
            // author-visible value expression (`T[K]`, `string`,
            // `Record<...>`, etc.).
            //
            // Whether THIS mapped root is such a carrier is a SEMANTIC
            // decision, so it is read off TYPED node-domain state — the
            // shape-engine fold's `RaisedRootKind::Mapped {
            // value_is_semantic_miss }` root class (derived from
            // `QueryError::Miss` through the shared sentinel authority)
            // via the node mirror [`node_root_is_published_operator`] —
            // never by matching the raised sentinel STRING. The carrier
            // is lowered once under a carrier-preserving
            // structural-transit demand (`may_reduce_operator == false`,
            // so the classification lowering never executes the mapped
            // type or enumerates its keys). A scope with no shallow
            // state has nothing to lower against (the whole-expression
            // materialiser bails to the input-unchanged path there), so
            // an unlowerable carrier stays a carrier-stop (`false`).
            let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
            dispatch
                .lower_type_expr_in_scope_with_context(
                    scope_canonical_id,
                    expr,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        mode,
                    ),
                )
                .is_some_and(|node| node_root_is_published_operator(ctx, node))
        }
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::TypeOf(_) => true,
        _ => false,
    }
}

/// Node-domain mirror of [`type_expr_root_is_published_operator`] applied to a
/// node's RAISED root term: is `node`'s NORMALIZED raised root a published
/// operator (`Ref` / `Mapped`-with-non-miss-value / `KeyOf` / `IndexedAccess` /
/// `Conditional` / `TypeOf`)?
///
/// Reads the POST-NORMALIZED raised root through the shared shape-engine fold
/// (`node_root_is_published_operator_with_dispatch`) — the SAME fold that drops
/// the Intersection sentinel / empty-object arms and peels the `Alias` identity
/// hops — so the answer equals `type_expr_root_is_published_operator(raise(node))`
/// BY CONSTRUCTION, including for shapes the former raw-node walk mis-classified
/// (e.g. `Intersection([{}, IndexedAccess])`, which the raw walk saw as a bare
/// `Intersection` ⇒ false but which raises to its operator arm ⇒ true). The
/// `Mapped` carrier-suppression (publish UNLESS the value root is EXACTLY the
/// `semanticMiss` sentinel) is carried in the fold's normalized root class, so the
/// NARROW miss-root rule still holds without a raw-node value re-fold here.
fn node_root_is_published_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_published_operator_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    node_root_is_published_operator_with_dispatch(&dispatch, node)
}

/// Whether `node`'s NORMALIZED raised ROOT term is a `TypeOf` — the node-domain
/// equivalent of `matches!(raise(node), TypeExpr::TypeOf(_))`. The registry
/// member-surface stabiliser scopes its typeof-result-miss admission refusal to a
/// `TypeOf`-rooted surface, exactly as
/// `materialize_component_meta_type_expr_until_stable_full` scopes that refusal to
/// a `TypeOf` input expr (so a miss-rooted `Pick`/`Omit`/`Ref` surface — a
/// different, already-handled class — is not refused here).
///
/// Reads the POST-NORMALIZED raised root through the shared shape-engine fold
/// (`node_root_is_typeof_with_dispatch`) — the SAME fold that drops the
/// Intersection sentinel / empty-object arms and peels the `Alias` hops — so the
/// answer equals the `TypeExpr` predicate on the raised value BY CONSTRUCTION,
/// including for shapes the former raw-node walk mis-classified (e.g.
/// `Intersection([{}, TypeOf])`, raw ⇒ false but raises to `TypeOf` ⇒ true).
fn node_root_is_typeof(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::project_semantic_dispatch::raise::node_root_is_typeof_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;

    let dispatch = ProjectSemanticDispatch::new(ctx);
    node_root_is_typeof_with_dispatch(&dispatch, node)
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

    // ONE cacheability tracer scope around the WHOLE compute — the input-node shape
    // facts, the reduction-context classification that builds the cache KEY, the
    // peek, and the cold reduce. Scoping it to the reduce alone left the
    // key-classification's and the shape-fact walk's serves unobserved.
    let (value, _non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
        ctx.host_for_fact_tracer_install(),
        |probe| {
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
                    // Snapshot the request-scoped materialization suppress sticky BEFORE the
                    // reduce, and take the scope observation BEFORE computing the value —
                    // mirroring the `TypeExpr`-start
                    // `materialize_component_meta_type_expr_until_stable_full`, so the
                    // signature self-root and the admission gate root on the version the
                    // reduce actually ran under (one tear-free observation taken before the
                    // value settles, not one re-read afterward).
                    let suppress_sticky_before =
                        crate::request_context::current_request_result_is_partial();
                    let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
                    // The cold reduce runs inside the caller's cacheability scope. A FENCED
                    // (ReturnOnly, `store_published == false`) `IndexedReady` serve consumed
                    // anywhere in this compute derives the member SHAPE from a
                    // served-without-publication basis while its fact signature validates
                    // against the LIVE view — a non-cacheable read this admission gate cannot
                    // otherwise reject. The `MaterializedOutputTypeExpr` carrier surfaces only
                    // `result_is_partial` (`raise.rs` deliberately folds a benign
                    // `cache_suppress` into the inner memo's own admission and NOT the
                    // carrier, so it MUST NOT suppress a complete component-meta result), so a
                    // fenced-but-`Complete` shape sails through the `result_is_partial()`-only
                    // gate below. The entry's own `ReadSetSignature` catches content-change
                    // supersessions and `validated_at_generation` catches generation-change
                    // ones; the residual hole the scope closes is the SAME-generation
                    // singleflight-race window, where admitting the shape would stale-serve it
                    // to a later same-generation warm hit.
                    //
                    // The scope's own observation set is NOT the entry's signature (the admit
                    // path below builds that from the carrier's `dep_signature`), so this
                    // boundary reads the scope's CACHEABILITY verdict — which folds the
                    // non-cacheable-read bit together with a fact-signature overflow (a second,
                    // INDEPENDENT non-admission condition that must not be dropped here).
                    let materialized = reduce_member_value_graph_native_with_context(
                        ctx,
                        scope_canonical_id,
                        first_node,
                        reduction_context,
                    );
                    // Reproduce ALL THREE of `_until_stable_full`'s admission rails, not just
                    // the partial gate:
                    //   1. a GENUINE-partial reduce (budget-tripped contributing read);
                    //   2. a reduce that observed a MissingDependency (the request's
                    //      materialization suppress sticky transitioned unset→set DURING this
                    //      reduce); and
                    //   3. a `typeof <unresolved import>`-rooted member surface whose reduced
                    //      ROOT is the unmaterialised/miss sentinel.
                    // Cases 2 + 3 are `ReturnOnly` partials whose only invalidation rail is
                    // the owner's `ImportRoute` derived fact — a rail this node-keyed slot's
                    // fact signature cannot carry — so admitting them warm would stale-serve
                    // the miss after the dependency appears. The value still flows to the
                    // caller; only the shared-slot admission is refused, and the next request
                    // recomputes cold and recovers. The suppress-sticky transition alone
                    // misses a typeof miss whose sticky an EARLIER `build_typeof` sub-read in
                    // the SAME request already set, so the typeof-root-miss check is carried
                    // IN ADDITION and is scoped to a `TypeOf`-rooted first-pass node (a
                    // miss-rooted `Pick`/`Omit`/`Ref` surface is a different, already-handled
                    // class the surrounding gates cover).
                    let observed_missing_dependency = !suppress_sticky_before
                        && crate::request_context::current_request_result_is_partial();
                    let typeof_result_root_is_miss = node_root_is_typeof(ctx, first_node)
                && materialized.node_id().is_some_and(|node| {
                    crate::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch(
                        &dispatch, node,
                    )
                });
                    if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                        materialized.result_is_partial(),
                    ) || observed_missing_dependency
                        || typeof_result_root_is_miss
                        || probe.non_cacheable()
                    {
                        materialized
                    } else {
                        let materialized_for_closure = materialized.clone();
                        let admitted = cache.get_or_compute(&key, ctx, probe, move || {
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
            let stable_has_miss = stable_facts.map(|f| !f.materialized());
            let first_has_miss = first_facts.map(|f| !f.materialized());
            if stable_has_miss == Some(true) && first_has_miss == Some(false) {
                RegistryMemberStabilizedValue::First { node: first_node }
            } else {
                RegistryMemberStabilizedValue::Stable { node: stable_node }
            }
        },
    );
    value
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

/// The TypeExpr-START shape route's ONE pre-peek lowering: the tear-free
/// scope observation plus the settled node the expression lowers to under
/// it. Shared by the whole-expression materialiser
/// ([`materialize_component_meta_type_expr_until_stable_full`], which keys,
/// peeks, cold-reduces, and admits off this ONE lowering) and the
/// projector peek ([`crate::meta_resolve::projectors::peek_member_shape_known`],
/// which must build the IDENTICAL key identity the materialiser publishes
/// under — a divergent lowering would peek a different node than the
/// publish keyed).
pub(crate) struct TypeExprShapeSubjectLowering {
    /// The tear-free scope observation the lowering ran under — the SAME
    /// observation the admit path self-roots the shared-cache entry on.
    /// `None` = the scope has no view-correct observation (the lowering
    /// degraded to the surviving `shallow_file_state` content version);
    /// shared-cache admission is skipped for that case.
    pub(crate) observed_scope: Option<crate::resolver_core::MaterializeScopeObservation>,
    /// The settled node the expression lowered to — the shape-cache
    /// subject AND the cold-reduce input.
    pub(crate) lowered: crate::semantic_query::SemanticNodeId,
}

/// Lower `expr` ONCE for the TypeExpr-START shape route: ONE tear-free
/// scope observation (`observe_materialize_scope` — overlay-aware,
/// view-correct) sources the lowering `NodeScopeId`'s `whole_hash`, so the
/// keyed subject node, the reduced value, and the admit fact-signature
/// self-root all agree on one scope content identity (sourcing them from
/// separate oracles tears: an edit landing between the reads roots a value
/// lowered under `H1` on a signature self-rooted at `H2`).
///
/// A `None` observation degrades to the scope's surviving
/// `shallow_file_state` content version — NEVER a fabricated all-zero hash
/// (the caller then skips shared-cache admission). When the scope has
/// neither an observation nor a surviving shallow state there is genuinely
/// no scope identity to lower against — returns `None` and the caller
/// produces its no-op result.
pub(crate) fn lower_type_expr_for_shape_subject(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    reduction_context: crate::semantic_query::ProjectionReductionContext,
) -> Option<TypeExprShapeSubjectLowering> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use std::sync::Arc;

    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    // Capture the scope-shadowing context once for the materialize → lower
    // pipeline from the per-scope memo, so the dispatch fast-path observes
    // the same shadow set the route-extraction path uses. The `&mut` borrow
    // ends here (the accessor returns an owned `Arc<ScopeShadowing>`), so
    // the shared `ctx` borrow opened just below — held through dispatch
    // lowering — is unaffected.
    let shadowing = query_engine.scope_shadowing_for_scope(scope_canonical_id);
    let ctx = query_engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
    let lowering_scope_whole_hash = match observed_scope.as_ref() {
        Some(observation) => Some(observation.whole_hash()),
        None => ctx
            .shallow_file_state(scope_canonical_id)
            .map(|state| state.whole_hash),
    };
    let scope = crate::semantic_query::NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash: lowering_scope_whole_hash?,
        local_scope: None,
    };
    let env: rustc_hash::FxHashMap<String, crate::semantic_query::SemanticNodeId> =
        rustc_hash::FxHashMap::default();
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    let _us_trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
    if _us_trace {
        eprintln!(
            "[US_LOWER_START] scope={} context={:?}",
            scope_canonical_id, reduction_context
        );
    }
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
            "[US_LOWER_END] scope={} context={:?} lower_ms={:.1}",
            scope_canonical_id, reduction_context, _us_lower_ms
        );
    }
    Some(TypeExprShapeSubjectLowering {
        observed_scope,
        lowered,
    })
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
    // ONE cacheability tracer scope around the WHOLE compute. This route's PRE-PEEK
    // LOWERING (`lower_type_expr_for_shape_subject`) resolves every nested reference
    // head through the shared carrier resolver's DIRECT `ensure_indexed_ready_serve`
    // probe, so a FENCED serve is consumed BEFORE the reduce ever runs — and for a
    // COMPOSITE subject the `StructuralTransit` reducer never descends into a
    // composite child, so the reduce is NOT guaranteed to re-read it. A tracer
    // scoped to the reduce alone therefore observed nothing and admitted the
    // poisoned shape. The scope must enclose the context classification, the
    // lowering, the keying, the peek, and the reduce.
    use crate::project_semantic_dispatch::output_materialization::{
        wrap_output_type_expr, OutputProjector,
    };
    use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use std::sync::Arc;

    // Step 6.2 / D22: count every entry into whole-expression
    // materialization. Memo hits + cold builds both increment so the
    // FAIL-FIRST test discriminates the call-ordering contract at the
    // *entry* boundary, not the build closure.
    #[cfg(test)]
    MTL_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let outer_ctx: &dyn crate::resolver_core::ResolverContext = query_engine.ctx;
    let (value, _non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
        outer_ctx.host_for_fact_tracer_install(),
        |probe| {
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
            let reduction_context = type_expr_materialize_reduction_context(
                query_engine.ctx(),
                scope_canonical_id,
                expr,
                mode,
            );

            // Snapshot the request-scoped materialization suppress sticky BEFORE
            // this compute lowers/reduces (the pre-peek shape-subject lowering
            // below is part of the compute). A `typeof <unresolved import>` is a
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
            let suppress_sticky_before =
                crate::request_context::current_request_result_is_partial();

            // ONE tear-free scope observation + ONE shallow lowering, shared by the
            // cache-key subject (the LOWERED settled node), the peek, the cold
            // reduce, and the admit self-root — so the keyed node, the value, and
            // the fact-signature self-root all agree on one scope content identity.
            // `None` = no view-correct scope identity to lower against (a session
            // tombstone, an evicted / unloaded scope, or no recoverable artifact):
            // the materialiser returns the input expression unchanged — the no-op
            // result the surrounding code already tolerates — and no cache slot is
            // keyed or peeked.
            let Some(shape_lowering) = lower_type_expr_for_shape_subject(
                query_engine,
                scope_canonical_id,
                expr,
                reduction_context,
            ) else {
                let ctx = query_engine.ctx();
                let dispatch = ProjectSemanticDispatch::new(ctx);
                let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
                return MaterializedOutputTypeExpr::from_parts(
                    None,
                    wrap_output_type_expr(&cap, expr.clone()),
                    Arc::from(Vec::new()),
                    false,
                );
            };
            let TypeExprShapeSubjectLowering {
                observed_scope,
                lowered,
            } = shape_lowering;

            // Classify the shape-cache key ONCE per materialization pass — over the
            // LOWERED settled node — and reuse the SAME `Option<ShapeCacheKey>` for
            // both the peek (below) and the admit (further down). `None` (a
            // composite NESTING a synthetic carrier — no sound content-free key)
            // still bypasses BOTH the peek and the admit; the value is computed and
            // returned either way.
            let cache_key =
                crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
                    std::sync::Arc::<str>::from(scope_canonical_id),
                    expr,
                    reduction_context,
                    || Some(lowered),
                );

            // Peek the universal ShapeCacheDb (member-value-node subject over the
            // pre-peek lowered node, whole-subject demand under the exact
            // reduction context).
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

            // Step 1.5 thin dispatch wrapper. The pre-peek shape-subject lowering
            // above already lowered the expression against the tear-free scope
            // observation; the cold path reduces that SAME settled node — no
            // second lowering.
            let ctx = query_engine.ctx();
            let dispatch = ProjectSemanticDispatch::new(ctx);
            // Mint the field-types materialiser output capability (constructor
            // visible only within `crate::meta_resolve::materialize::field_types`):
            // this materialiser is a true publication sink — it reduce-then-raises
            // into a sealed carrier and unwraps the sealed payload via the capability.
            let cap = MetaResolveFieldTypesOutputCap::new(&dispatch);
            let _us_trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
            let _us_rr_t0 = Instant::now();
            // The cold reduce runs inside the caller's cacheability scope, alongside the
            // pre-peek lowering. A FENCED (ReturnOnly, `store_published == false`)
            // `IndexedReady` serve consumed anywhere in the compute derives this shape from
            // a served-without-publication basis while its fact signature validates against
            // the LIVE view, and a signature OVERFLOW leaves the compute unprovable against
            // the curated signature — two independent non-cacheable states that are NOT
            // partial, so the `result_is_partial()` gate below cannot reject either. The
            // admit builds its signature from the carrier's `dep_signature`, never from the
            // scope's observation set, so the boundary reads the CACHEABILITY verdict; the
            // value still flows to the caller, only the shared `ShapeCacheDb` write is
            // refused.
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
            // triple. A NON-CACHEABLE read (a fenced ReturnOnly serve, a broken
            // decl-body lease, an unrootable route) and a tracer signature OVERFLOW
            // do NOT set `result_is_partial` — they are COMPLETE but unrootable — so
            // this partial gate cannot reject them; they are refused on the separate
            // CACHEABILITY rail the enclosing tracer scope produces. The
            // freshly-computed value is always returned to the caller; only the
            // shared-cache admission is refused.
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
            let observed_missing_dependency = !suppress_sticky_before
                && crate::request_context::current_request_result_is_partial();
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
                && !probe.non_cacheable()
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
                        let _ = host_db.get_or_compute(cache_key, ctx, probe, move || {
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
        },
    );
    value
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
/// resolved by the node front — NEVER a name re-resolved from `scope`), decide
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
    // The engine hands back the decl's content-free authored-body LOCATOR;
    // lower it through the ONE shared dispatch (transit demand — carrier-
    // preserving) and read the object-surface verdict off the lowered node.
    let verdict = query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .and_then(|locator| {
            let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
                query_engine.ctx,
            );
            dispatch.raise_authored_locator_to_hot(
                &locator,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            )
        })
        .is_some_and(|hot| {
            crate::resolver_core::component_meta_query_engine::component_meta_registry_node_has_explicit_object_surface(
                query_engine.ctx,
                hot.node(),
            )
        });
    if refused {
        return (verdict, None);
    }
    let fence_sig: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
    (verdict, Some(fence_sig))
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

#[cfg(test)]
#[path = "field_types_tests.rs"]
mod stabilizer_admission_tests;
