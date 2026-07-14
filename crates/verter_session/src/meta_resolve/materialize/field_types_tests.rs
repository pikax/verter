//! `#[cfg(test)]` module for the registry-member shape stabiliser
//! (`stabilize_registry_member_surface_node_with_shape_cache`) and the node-domain
//! `typeof`-root predicate — extracted to a sibling `_tests.rs` (excluded from the
//! oversize-files guard) so the production module stays under the line cap. The
//! module is a descendant of `field_types`, so `super::` reaches its private items.

use std::sync::Arc;

use super::{
    node_materialize_reduction_context, stabilize_registry_member_surface_node_with_shape_cache,
    RegistryMemberShapeKeyCap,
};
use crate::component_meta_caches::ShapeCacheKey;
use crate::meta::MetaProject;
use crate::semantic_query::{
    PrimitiveKind, ProjectionMode, ScopeId, SemanticNodeData, SemanticNodeId, ValueRootKey,
};
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;

/// Peek the ShapeCacheDb member-VALUE-node slot the stabiliser keys, reconstructing
/// the key the SAME way `stabilize_registry_member_surface_node_with_shape_cache`
/// does (the EXACT `node_materialize_reduction_context` + member-value-node key).
fn slot_warm(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope: &str,
    first_node: SemanticNodeId,
) -> bool {
    let reduction_context =
        node_materialize_reduction_context(ctx, first_node, ProjectionMode::Navigate);
    let cap = RegistryMemberShapeKeyCap::new();
    let key = ShapeCacheKey::registry_member_value_node_whole_with_context(
        Arc::<str>::from(scope),
        &cap,
        first_node,
        reduction_context,
    );
    ctx.project_type_store()
        .shape_cache_db()
        .peek(&key, ctx)
        .is_some()
}

/// The stabiliser carries `_until_stable_full`'s extra admission rails
/// (`typeof_result_root_is_miss` + `observed_missing_dependency`), and this test
/// PROVES — discriminatingly — that the typeof-miss stale-serve the rails defend
/// against is UNREACHABLE in the registry-member (Navigate) stabiliser path, because
/// a `TypeOf` root reduces to a MATERIALISED deferred carrier (NOT a cached miss
/// sentinel) under the stabiliser's `Published(Navigate)` context. A deferred carrier
/// admitted to the slot re-resolves the typeof on demand (correct), so it is NOT the
/// import-route-rail-less cached miss that would stale-serve.
///
/// Two discriminating assertions:
/// 1. the new `node_root_is_typeof` helper TRUE for a `TypeOf` root, FALSE for a
///    non-`TypeOf` root (the typeof-scope the rail keys on);
/// 2. the typeof root reduces to a MATERIALISED non-sentinel (a deferred carrier) at
///    the stabiliser's Navigate context — so `typeof_result_root_is_miss` is
///    correctly NOT tripped and the carrier is admitted warm. If `Navigate` lowering
///    ever STARTED resolving a typeof to a cached miss here, assertion (2) fails and
///    surfaces that the refusal rail became load-bearing.
#[test]
fn typeof_root_reduces_to_deferred_carrier_so_stale_serve_is_unreachable() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let graph = ctx.project_type_store().semantic_graph();

    // first_node = `typeof definitelyMissingValue` — a TypeOf carrier whose value
    // root is unresolvable in /p.ts.
    let typeof_node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/p.ts"),
                local_scope: None,
            },
            name: Arc::from("definitelyMissingValue"),
        },
        Arc::from(Vec::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    let primitive_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // (1) The typeof-scope helper the rail keys on discriminates a TypeOf root.
    assert!(
        super::node_root_is_typeof(ctx, typeof_node),
        "node_root_is_typeof must be TRUE for a TypeOf root (the rail's scope)",
    );
    assert!(
        !super::node_root_is_typeof(ctx, primitive_node),
        "node_root_is_typeof must be FALSE for a non-TypeOf root",
    );

    // (2) The typeof reduces to a MATERIALISED non-sentinel (a deferred carrier) at
    // the stabiliser's `Published(Navigate)` context — NOT a cached miss — so
    // `typeof_result_root_is_miss` is correctly not tripped and the carrier is
    // admitted warm (re-resolves on demand; no stale-serve).
    let reduction_context =
        node_materialize_reduction_context(ctx, typeof_node, ProjectionMode::Navigate);
    let reduced = super::reduce_member_value_graph_native_with_context(
        ctx,
        "/p.ts",
        typeof_node,
        reduction_context,
    );
    let result_is_sentinel = reduced.node_id().is_some_and(|n| {
        crate::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch(
            &dispatch, n,
        )
    });
    assert!(
        !result_is_sentinel,
        "a typeof root reduces to a deferred carrier (NOT a cached miss) under \
         Published(Navigate); the typeof-miss stale-serve is unreachable in this path. \
         If this fails, the typeof now resolves to a miss here and the \
         typeof_result_root_is_miss refusal rail became load-bearing",
    );

    let _ = stabilize_registry_member_surface_node_with_shape_cache(
        ctx,
        "/p.ts",
        typeof_node,
        ProjectionMode::Navigate,
    );
    assert!(
        slot_warm(ctx, "/p.ts", typeof_node),
        "a deferred typeof carrier IS admitted warm (it re-resolves on demand — correct, \
         not the import-route-rail-less cached miss the rails refuse)",
    );
}

/// Intern the `typeof definitelyMissingValue` subject in `ctx`'s graph — the
/// SAME node shape [`typeof_root_reduces_to_deferred_carrier_so_stale_serve_is_unreachable`]
/// proves reduces to a deferred carrier that IS admitted warm (so a control run
/// is a genuine admission, not a degenerate refusal), and whose reduce drives an
/// `ensure_indexed_ready_serve` for `/p.ts` (so the fence has a serve to catch).
fn intern_typeof_subject(ctx: &dyn crate::resolver_core::ResolverContext) -> SemanticNodeId {
    ctx.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::new_typeof(
            ValueRootKey {
                scope: ScopeId {
                    canonical_id: Arc::from("/p.ts"),
                    local_scope: None,
                },
                name: Arc::from("definitelyMissingValue"),
            },
            Arc::from(Vec::new().into_boxed_slice()),
            Arc::from(Vec::new().into_boxed_slice()),
        ))
}

/// Inner-cache fenced-serve poison — [`ShapeCacheDb`] must REFUSE
/// admission when the per-member cold reduce
/// ([`reduce_member_value_graph_native_with_context`]) consumed a FENCED
/// (ReturnOnly, `store_published == false`) `IndexedReady` serve.
///
/// A fenced serve is non-cacheable but NOT partial, and the
/// `MaterializedOutputTypeExpr` carrier surfaces only `result_is_partial`
/// (`raise.rs` deliberately folds a benign `cache_suppress` into the inner
/// memo's OWN admission, never the carrier — it MUST NOT suppress a complete
/// component-meta result), so the stabiliser's `result_is_partial()`-only gate
/// (plus the `observed_missing_dependency` + `typeof_result_root_is_miss` rails,
/// none of which fire for a deferred carrier) does NOT catch it. The ONLY rail
/// that refuses the poisoned shape is the nested fact tracer wrapping the cold
/// reduce (the `RefCycleResultDb` / `app_config_no_override_proof` /
/// `ResolvabilityDb::can_resolve_registry_symbol` sibling pattern).
///
/// DISCRIMINATING: `force_indexed_ready_serve_fence_for_tests` fences every
/// `ensure_indexed_ready_serve` the reduce drives at a STABLE generation (no
/// bump — so a `GenerationSuperseded` gate cannot mask the refusal, and the
/// served `indexed` still reduces the shape to the SAME deferred carrier). The
/// unfenced control admits the shape (`slot_warm` TRUE, `live_count` grows); the
/// fenced request must NOT (`slot_warm` FALSE, `live_count` unchanged) while the
/// shape stays `Complete` (the fenced serve routes through the fact tracer, never
/// the request partial sticky). RED-pre (drop the `|| non_cacheable_read_observed`
/// gate clause) the fenced shape LANDS in `ShapeCacheDb` and a later
/// same-generation warm hit inherits the stale shape derived from a
/// served-without-publication basis.
#[test]
fn fenced_serve_shape_cache_member_value_is_not_admitted() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::sync::atomic::Ordering;

    // Control — an UNFENCED member-value reduce admits the deferred carrier.
    let control_host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let control_project = MetaProject::new(control_host);
    control_project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let control_session = control_project.open_session_batch().unwrap();
    let _ = control_session.evaluate_types("/p.ts").unwrap();
    let control_ctx: &dyn crate::resolver_core::ResolverContext = control_session.host();
    let control_node = intern_typeof_subject(control_ctx);
    let control_before = control_ctx
        .project_type_store()
        .shape_cache_db()
        .live_count();
    let _ = stabilize_registry_member_surface_node_with_shape_cache(
        control_ctx,
        "/p.ts",
        control_node,
        ProjectionMode::Navigate,
    );
    assert!(
        slot_warm(control_ctx, "/p.ts", control_node),
        "fixture invariant: an unfenced member-value reduce admits the deferred \
         carrier into ShapeCacheDb (otherwise the fenced assertion is vacuous)",
    );
    assert!(
        control_ctx
            .project_type_store()
            .shape_cache_db()
            .live_count()
            > control_before,
        "fixture invariant: the control admission grows ShapeCacheDb live_count",
    );

    // Fenced — every `ensure_indexed_ready_serve` the reduce drives is fenced at
    // a STABLE generation, so the shape is derived from a served-without-
    // publication artifact while its facts validate against the live view.
    let fenced_host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let fenced_project = MetaProject::new(fenced_host);
    fenced_project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let fenced_session = fenced_project.open_session_batch().unwrap();
    let _ = fenced_session.evaluate_types("/p.ts").unwrap();
    let fenced_ctx: &dyn crate::resolver_core::ResolverContext = fenced_session.host();
    let fenced_node = intern_typeof_subject(fenced_ctx);
    let fenced_before = fenced_ctx
        .project_type_store()
        .shape_cache_db()
        .live_count();
    {
        let rctx = RequestContext::new(1, Arc::from("/p.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced_session
            .host()
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let _ = stabilize_registry_member_surface_node_with_shape_cache(
            fenced_ctx,
            "/p.ts",
            fenced_node,
            ProjectionMode::Navigate,
        );
        fenced_session
            .host()
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        // HARD FLOOR: a fenced serve is non-cacheable, NOT partial — it must NOT
        // raise the request partial sticky. Non-cacheability routes through the
        // fact tracer; the shape stays `Complete`.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced member-value serve is non-cacheable, NOT partial — the shape \
             stays Complete; non-cacheability routes through the fact tracer, never \
             the partial sticky",
        );
    }
    assert!(
        !slot_warm(fenced_ctx, "/p.ts", fenced_node),
        "POISON: a fenced (non-cacheable) member-value reduce admitted its shape into \
         ShapeCacheDb — the nested fact tracer (RefCycleResultDb / app_config / \
         ResolvabilityDb sibling pattern) must refuse admission, else a later \
         same-generation warm hit inherits the stale shape derived from a \
         served-without-publication basis",
    );
    assert_eq!(
        fenced_ctx
            .project_type_store()
            .shape_cache_db()
            .live_count(),
        fenced_before,
        "POISON: the fenced reduce grew ShapeCacheDb live_count — the fenced shape was \
         admitted despite being non-cacheable",
    );
}

/// The SAME registry-member `ShapeCacheDb` admission boundary must ALSO refuse on
/// a tracer `FactReadSetFinalise::Overflow` — the SECOND, independent
/// non-admission condition. The admit builds its signature from the carrier's
/// `dep_signature`, NOT from the cold-reduce tracer's finalised set, so an overflow
/// seen only by the tracer would be dropped on the floor and a ROOTLESS entry would
/// warm the shared cache: an observation set above `FACT_SIGNATURE_CAP` can be
/// rooted by no signature, so a warm read could never revalidate it.
///
/// DISCRIMINATING: the per-host overflow knob fans `FACT_SIGNATURE_CAP + 1`
/// synthetic observations into every installed tracer, so the reduce's tracer
/// finalises `Overflow` with NO fenced serve and NO partial — the exact state the
/// pre-fix boundary (which read only `non_cacheable_read_observed` and discarded the
/// finalise) ADMITTED. The unarmed control admits (`slot_warm`); the overflowed
/// reduce must NOT.
#[test]
fn tracer_overflow_refuses_registry_member_shape_admission() {
    use std::sync::atomic::Ordering;

    // Control — an unarmed reduce admits the deferred carrier.
    let control_host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let control_project = MetaProject::new(control_host);
    control_project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let control_session = control_project.open_session_batch().unwrap();
    let _ = control_session.evaluate_types("/p.ts").unwrap();
    let control_ctx: &dyn crate::resolver_core::ResolverContext = control_session.host();
    let control_node = intern_typeof_subject(control_ctx);
    let _ = stabilize_registry_member_surface_node_with_shape_cache(
        control_ctx,
        "/p.ts",
        control_node,
        ProjectionMode::Navigate,
    );
    assert!(
        slot_warm(control_ctx, "/p.ts", control_node),
        "fixture invariant: an unarmed member-value reduce admits the deferred carrier \
         into ShapeCacheDb (otherwise the overflow assertion is vacuous)",
    );

    // Overflowed — the reduce's tracer observes above the cap.
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let ctx: &dyn crate::resolver_core::ResolverContext = session.host();
    let node = intern_typeof_subject(ctx);
    session
        .host()
        .test_force
        .force_fact_tracer_overflow_observations
        .store(
            crate::resolver_core::FACT_SIGNATURE_CAP + 1,
            Ordering::Relaxed,
        );
    let _ = stabilize_registry_member_surface_node_with_shape_cache(
        ctx,
        "/p.ts",
        node,
        ProjectionMode::Navigate,
    );
    session
        .host()
        .test_force
        .force_fact_tracer_overflow_observations
        .store(0, Ordering::Relaxed);
    assert!(
        !slot_warm(ctx, "/p.ts", node),
        "POISON: a signature-OVERFLOWED member-value reduce admitted its shape into \
         ShapeCacheDb — an observation set above FACT_SIGNATURE_CAP can be rooted by no \
         signature, so the entry could never be revalidated on a warm read. Overflow must \
         refuse INDEPENDENTLY at this tracer boundary (pre-fix the `_finalise` was \
         discarded)",
    );
}

/// DEPTH regression: `node_root_is_typeof` follows an `Alias` chain DEEPER than
/// the former fixed depth cap (32) to reach the `TypeOf` root. The visited-set
/// termination walks an acyclic chain of ANY depth.
///
/// MUTATION-PROOF: reinstating a `MAX_DEPTH = 32` cap stops the walk before the
/// 40-deep `TypeOf` terminal, so `node_root_is_typeof` returns false instead of
/// true and the first assertion FAILS.
#[test]
fn node_root_is_typeof_walks_deep_alias_chain_without_depth_cutoff() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let graph = ctx.project_type_store().semantic_graph();

    // 40 > the former 32 cap.
    const DEPTH: usize = 40;
    let typeof_terminal = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/p.ts"),
                local_scope: None,
            },
            name: Arc::from("definitelyMissingValue"),
        },
        Arc::from(Vec::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    let mut deep_typeof = typeof_terminal;
    for _ in 0..DEPTH {
        deep_typeof = graph.intern_node(SemanticNodeData::Alias(deep_typeof));
    }
    assert!(
        super::node_root_is_typeof(ctx, deep_typeof),
        "node_root_is_typeof must follow a >32-deep alias chain to the TypeOf root \
         (a reinstated MAX_DEPTH=32 stops short and returns false)",
    );

    // Anti-vacuity: a deep alias chain terminating in a NON-TypeOf root is not a
    // typeof root (the visited-set walk reaches the terminal and rejects it).
    let primitive_terminal = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mut deep_primitive = primitive_terminal;
    for _ in 0..DEPTH {
        deep_primitive = graph.intern_node(SemanticNodeData::Alias(deep_primitive));
    }
    assert!(
        !super::node_root_is_typeof(ctx, deep_primitive),
        "a deep alias chain terminating in a non-TypeOf root is NOT a typeof root",
    );
}

/// The whole-`TypeExpr` shape route
/// ([`super::materialize_component_meta_type_expr_until_stable_full`], keyed under
/// `ShapeCacheKey::type_expr_whole_with_context`) must refuse `ShapeCacheDb`
/// admission when its cold reduce consumed a NON-CACHEABLE read — the same rail its
/// node-start twins (the registry-member stabiliser above, the surface-member sink)
/// already carry.
///
/// A FENCED (ReturnOnly, `store_published == false`) serve is non-cacheable but NOT
/// partial, and the sealed carrier surfaces only `result_is_partial`, so this
/// route's three admission rails (`result_is_partial` / `observed_missing_dependency`
/// / `typeof_result_root_is_miss`) cannot reject it: a fenced-but-`Complete` shape
/// sailed straight into the shared slot, where a later same-generation warm peek
/// (`peek_member_shape_known`) inherits it.
///
/// DISCRIMINATING: `force_indexed_ready_serve_fence_for_tests` fences every
/// `ensure_indexed_ready_serve` the reduce drives at a STABLE generation (no bump —
/// so a `GenerationSuperseded` gate cannot mask the refusal, and the served `indexed`
/// still reduces the shape to the SAME deferred carrier). The unfenced control admits
/// (`live_count` grows); the fenced materialise must NOT, while the request stays
/// `Complete`. RED-pre (drop the `!reduce_non_cacheable` clause) the fenced shape
/// LANDS in `ShapeCacheDb`.
#[test]
fn fenced_serve_type_expr_whole_shape_is_not_admitted() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::ComponentMetaQueryEngine;
    use std::sync::atomic::Ordering;

    /// `Anchor['x']` — an indexed access whose reduce MUST load `Anchor`'s
    /// declaration body (so it drives an `ensure_indexed_ready_serve` and the fence
    /// has a serve to catch) and settles a concrete leaf that IS admitted warm (so the
    /// control is a genuine admission, not a degenerate refusal).
    fn subject_expr() -> verter_type_expr::TypeExpr {
        verter_type_expr::TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(verter_type_expr::TypeExpr::named("Anchor")),
            index: std::sync::Arc::new(verter_type_expr::TypeExpr::string_literal("x")),
        }
    }

    fn drive(host: &VerterHost) -> usize {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let _ = super::materialize_component_meta_type_expr_until_stable_full(
            &subject_expr(),
            "/p.ts",
            ProjectionMode::Navigate,
            &mut engine,
        );
        host.project_type_store().shape_cache_db().live_count()
    }

    // Control — an UNFENCED whole-TypeExpr materialise admits the deferred carrier.
    let control = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let control_project = MetaProject::new(control);
    control_project
        .upsert_base("/p.ts", "export type Anchor = { x: number }\n")
        .unwrap();
    let control_session = control_project.open_session_batch().unwrap();
    let _ = control_session.evaluate_types("/p.ts").unwrap();
    let control_host = control_session.host();
    let control_before = control_host
        .project_type_store()
        .shape_cache_db()
        .live_count();
    let control_after = drive(control_host);
    assert!(
        control_after > control_before,
        "fixture invariant: an unfenced whole-TypeExpr materialise ADMITS its shape into \
         ShapeCacheDb (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` the reduce drives is fenced at a
    // STABLE generation.
    let fenced = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let fenced_project = MetaProject::new(fenced);
    fenced_project
        .upsert_base("/p.ts", "export type Anchor = { x: number }\n")
        .unwrap();
    let fenced_session = fenced_project.open_session_batch().unwrap();
    let _ = fenced_session.evaluate_types("/p.ts").unwrap();
    let fenced_host = fenced_session.host();
    let fenced_before = fenced_host
        .project_type_store()
        .shape_cache_db()
        .live_count();
    let fenced_after = {
        let rctx = RequestContext::new(1, Arc::from("/p.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced_host
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let after = drive(fenced_host);
        fenced_host
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        // HARD FLOOR: a fenced serve is non-cacheable, NOT partial.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced whole-TypeExpr serve is non-cacheable, NOT partial — the shape stays \
             Complete; non-cacheability routes through the fact tracer, never the sticky",
        );
        after
    };
    assert_eq!(
        fenced_after, fenced_before,
        "POISON: a fenced (non-cacheable) whole-TypeExpr materialise admitted its shape into \
         ShapeCacheDb — the nested fact tracer wrapping the cold reduce must refuse admission \
         (the `result_is_partial` / missing-dependency / typeof-miss rails cannot: a fenced \
         serve is Complete), else a later same-generation warm peek inherits the stale shape \
         derived from a served-without-publication basis",
    );
}

/// The tracer must enclose the WHOLE compute, not just the cold reduce.
///
/// The whole-`TypeExpr` route classifies its reduction context, LOWERS the
/// expression (`lower_type_expr_for_shape_subject`), keys, and peeks BEFORE it
/// reduces. The lowering resolves every nested reference head through the shared
/// carrier resolver's DIRECT `ensure_indexed_ready_serve` probe — so a FENCED
/// (ReturnOnly, `store_published == false`) serve is consumed by the LOWERING.
///
/// For a COMPOSITE subject (a `Union` whose arm is a `Ref`) under the
/// `StructuralTransit(Navigate)` context, the reducer does NOT descend into
/// composite children (the parent is the demand terminal), so the reduce NEVER
/// re-reads the fenced declaration. A tracer that starts at the reduce therefore
/// observes nothing and the fenced-but-`Complete` shape is ADMITTED into the shared
/// `ShapeCacheDb`, where a later same-generation warm peek inherits a shape derived
/// from a served-without-publication basis.
///
/// The root `Anchor['x']` subject cannot expose this: its reduction necessarily
/// re-reads `Anchor`, so a reduce-only tracer happens to catch the fence. This
/// composite subject is the route that has no such accident.
///
/// DISCRIMINATING: the unfenced control ADMITS (`live_count` grows — so the fenced
/// assertion is not vacuous); the fenced run must NOT admit, while the request stays
/// `Complete` (non-cacheability is orthogonal to partiality). RED against a tree
/// whose cacheability tracer starts at the reduce instead of enclosing the lowering.
#[test]
fn fenced_serve_in_pre_reduce_lowering_refuses_composite_shape_admission() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::ComponentMetaQueryEngine;
    use std::sync::atomic::Ordering;

    /// `Anchor | string` — a COMPOSITE whose nested `Ref` arm forces the pre-peek
    /// lowering to resolve (and serve) `Anchor`'s declaring file, while the
    /// `StructuralTransit(Navigate)` reduce keeps the union terminal and never
    /// descends into the arm.
    fn subject_expr() -> verter_type_expr::TypeExpr {
        verter_type_expr::TypeExpr::Union(Arc::from(
            vec![
                verter_type_expr::TypeExpr::named("Anchor"),
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            ]
            .into_boxed_slice(),
        ))
    }

    fn drive(host: &VerterHost) -> usize {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let _ = super::materialize_component_meta_type_expr_until_stable_full(
            &subject_expr(),
            "/p.ts",
            ProjectionMode::Navigate,
            &mut engine,
        );
        host.project_type_store().shape_cache_db().live_count()
    }

    fn fixture() -> crate::meta::MetaSession {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        });
        let project = MetaProject::new(host);
        project
            .upsert_base("/p.ts", "export type Anchor = { x: number }\n")
            .unwrap();
        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/p.ts").unwrap();
        session
    }

    // Control — an UNFENCED composite materialise admits its shape.
    let control_session = fixture();
    let control_host = control_session.host();
    let control_before = control_host
        .project_type_store()
        .shape_cache_db()
        .live_count();
    let control_after = drive(control_host);
    assert!(
        control_after > control_before,
        "fixture invariant: an unfenced composite (`Anchor | string`) materialise ADMITS \
         its shape into ShapeCacheDb (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` is fenced at a STABLE generation,
    // so the PRE-REDUCE lowering of the nested `Ref` arm consumes a fenced serve.
    let fenced_session = fixture();
    let fenced_host = fenced_session.host();
    let fenced_before = fenced_host
        .project_type_store()
        .shape_cache_db()
        .live_count();
    let fenced_after = {
        let rctx = RequestContext::new(1, Arc::from("/p.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced_host
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let after = drive(fenced_host);
        fenced_host
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        // HARD FLOOR: a fenced serve is non-cacheable, NOT partial.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced serve consumed by the pre-reduce lowering is non-cacheable, NOT \
             partial — the shape stays Complete",
        );
        after
    };
    assert_eq!(
        fenced_after, fenced_before,
        "POISON: a fenced serve consumed by the PRE-REDUCE LOWERING admitted its composite \
         shape into ShapeCacheDb. The `StructuralTransit` reducer never descends into a \
         composite child, so a tracer that starts at the reduce cannot re-observe the fenced \
         read — the cacheability tracer MUST enclose the WHOLE compute (context \
         classification, lowering, keying, peek, reduce), not just the reduce",
    );
}
