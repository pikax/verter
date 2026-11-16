//! The fallthrough caches must never warm-admit a NON-CACHEABLE compute.
//!
//! `FallthroughResolverState::store_node` — and the sibling `cached_fallthrough`
//! mirror on `DerivedRawState` — admit into caches whose entries root on the LIVE
//! view. A compute that consumed a FENCED (ReturnOnly, `store_published == false`)
//! `IndexedReady` serve — or a broken decl-body lease, an unrootable import route,
//! an unobservable contributor source env — produced its value from a
//! served-without-publication basis while its fact stamps read the live view.
//!
//! That is the poison, and it is PERMANENT: three of those four reasons are
//! CONTENT-NEUTRAL. The artifact stays published and content-current, so the
//! admitted entry's facts VALIDATE on every warm read, forever. No later edit
//! moves a hash that would evict it. "It re-resolves eventually" is false here.
//!
//! The rail is the unforgeable [`CacheabilityProbe`](verter_type_engine::fact_signature_helpers::CacheabilityProbe):
//! the funnels REQUIRE one and sample it AFTER the compute, and its scope
//! ENCLOSES that compute. The completeness rail cannot substitute for it — a
//! fenced serve is `Complete` by construction (non-cacheability is never
//! partiality), so `current_cold_compute_completeness()` reads `Complete` and
//! admits.
//!
//! # Discrimination contract
//!
//! `force_indexed_ready_serve_fence_for_tests` fences every
//! `ensure_indexed_ready_serve` the fallthrough compute drives, at a STABLE
//! generation — no `project_generation` bump, so a generation gate cannot mask the
//! refusal, and the served `indexed` still resolves the surface. The fixture's
//! root is an IMPORTED child component, so the compute genuinely crosses the file
//! boundary and reaches a serve.
//!
//! Three arms, all needed:
//!
//! - **control** — an unfenced compute ADMITS every node below. Anti-vacuity: the
//!   refusal arm is not passing on a compute that never reached the funnel.
//! - **fenced** — every node whose compute consumed the fence is REFUSED, while
//!   the caller is still SERVED its resolution and the request stays `Complete`.
//! - **path-precision** — the fence does NOT blanket-refuse: nodes whose OWN
//!   compute was provably fence-free (their local scope's probe stayed clean)
//!   still admit. A rail that refused everything while the knob is armed would
//!   pass the fenced arm vacuously.
//!
//! Drop the `probe.non_cacheable()` refusal from `store_node` and the fenced nodes
//! LAND — which is exactly the state this file characterises as poison.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::resolver_core::fallthrough_resolver::{child_surface_key, root_follow_key};
use crate::resolver_core::{
    fallthrough_cache_key, FallthroughNodeKey, FallthroughOverrideIdentity, FallthroughRequestHost,
};
use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// A parent SFC whose single template root is an IMPORTED child component.
///
/// The child root forces the fallthrough compute through the real cross-file path
/// — import-route resolution plus the child's own surface follow — which is where
/// `ensure_indexed_ready_serve` is driven and therefore where the fence lands. A
/// native-only root resolves entirely from the static intrinsic catalog and never
/// reaches a serve, which would make the fenced arm vacuous.
fn build_child_root_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Link.vue",
        r#"<script setup lang="ts">
defineProps<{ href: string }>()
</script>
<template><a :href="href"><slot /></a></template>"#,
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script setup lang="ts">
import Link from './Link.vue'
defineProps<{ label: string }>()
</script>
<template><Link :href="label" /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./Link.vue".to_string(),
            resolved_canonical_id: Some("/src/Link.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    // Publish both artifacts BEFORE the fence is armed, so the fenced run
    // exercises a fenced SERVE of a PUBLISHED artifact — the content-neutral
    // case — not a missing one.
    assert!(host.ensure_indexed_ready("/src/Link.vue").is_some());
    assert!(host.ensure_indexed_ready("/src/Button.vue").is_some());
    host
}

/// The fallthrough nodes whose compute CROSSES the file boundary, and therefore
/// consumes the fenced `IndexedReady` serve: the owner's root-follow and
/// branch-union surfaces, the child's surface follow, and the child's own
/// top-level surfaces (the child is resolved recursively through the same
/// engine).
fn fence_reaching_keys(host: &VerterHost) -> Vec<(&'static str, FallthroughNodeKey)> {
    let generic = FallthroughRequestHost::generic_root_propagation(host);
    let none = FallthroughOverrideIdentity::for_overrides(None);
    vec![
        (
            "Button root-follow",
            root_follow_key("/src/Button.vue", none.clone(), generic),
        ),
        (
            "Button branch-union",
            fallthrough_cache_key("/src/Button.vue", generic, None),
        ),
        (
            "Link child-surface-follow",
            child_surface_key("/src/Link.vue", none.clone()),
        ),
        (
            "Link root-follow",
            root_follow_key("/src/Link.vue", none, generic),
        ),
        (
            "Link branch-union",
            fallthrough_cache_key("/src/Link.vue", generic, None),
        ),
    ]
}

fn candidates(host: &VerterHost, key: &FallthroughNodeKey) -> usize {
    host.resolver_runtime()
        .fallthrough
        .cached_candidate_count(key)
}

fn node_count(host: &VerterHost) -> usize {
    host.resolver_runtime().fallthrough.cached_node_count()
}

fn mirror_present(host: &VerterHost, canonical: &str) -> bool {
    host.derived_raw_cache()
        .get(canonical)
        .is_some_and(|entry| entry.cached_fallthrough.is_some())
}

#[test]
fn resolve_only_partial_fallthrough_is_not_admitted_and_reaches_caller_scope() {
    use verter_type_engine::request_context::{
        current_cold_compute_completeness, ColdComputeCompletenessScope, RequestContext,
        RequestContextGuard,
    };
    use verter_type_engine::semantic_query::ResultCompleteness;

    const OWNER: &str = "/src/Owner.vue";
    let build_host = || {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        });
        upsert_vue(
            &host,
            OWNER,
            r#"<script setup lang="ts">
defineProps<Partial<{ label: string; count: number }>>()
</script>
<template><div /></template>"#,
        );
        // Prime only the real shallow DTO. Resolve's later evaluated-field
        // projectors still have cold work; the surface walk can reuse this DTO.
        crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
            let dispatch =
                verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
            let snapshot = host
                .get_raw_analysis_snapshot(OWNER)
                .expect("owner snapshot");
            let scope = ColdComputeCompletenessScope::enter();
            let macros = crate::resolver_core::component_meta_resolved_macros(
                &host,
                ctx,
                &dispatch,
                OWNER,
                &snapshot.macros,
            );
            assert!(!macros.is_empty());
            assert_eq!(
                current_cold_compute_completeness(),
                ResultCompleteness::Complete
            );
            scope.discard();
        });
        host
    };
    let budget_context = || {
        RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from(OWNER),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1,
        )
    };

    // Establish that only resolve produces the partiality. An independently
    // scoped surface walk under the SAME exhausted budget does not repeat it.
    let probe = build_host();
    let resolve_completeness = {
        let request = budget_context();
        let _guard = RequestContextGuard::install(Arc::clone(&request));
        let caller_scope = ColdComputeCompletenessScope::enter();
        let completeness = crate::resolver_core::with_bare_host_ctx_for_test(&probe, |ctx| {
            let dispatch =
                verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
            let resolved = probe
                .compute_component_meta_state_for_fallthrough(
                    OWNER,
                    probe.current_or_read_whole_hash(OWNER).expect("owner hash"),
                    ctx,
                    &dispatch,
                )
                .expect("owner resolve");
            assert!(request.projection_budget.is_exhausted());
            assert!(
                resolved.completeness.is_partial(),
                "{:?}",
                resolved.completeness
            );
            assert_eq!(
                current_cold_compute_completeness(),
                ResultCompleteness::Complete
            );
            let mut visiting = rustc_hash::FxHashSet::default();
            assert!(probe
                .compute_fallthrough_surface_from_resolved_state(
                    OWNER,
                    &resolved,
                    None,
                    &mut visiting,
                    ctx,
                    &dispatch,
                )
                .is_some());
            assert_eq!(
                current_cold_compute_completeness(),
                ResultCompleteness::Complete
            );
            let outcome = probe.compute_fallthrough_outcome_from_resolved_state(
                OWNER,
                &resolved,
                None,
                &mut visiting,
                ctx,
                &dispatch,
            );
            assert!(outcome.resolution.is_some());
            assert_eq!(outcome.completeness, resolved.completeness);
            assert_eq!(current_cold_compute_completeness(), resolved.completeness);
            resolved.completeness
        });
        caller_scope.discard();
        completeness
    };

    let host = build_host();
    let request = budget_context();
    let _guard = RequestContextGuard::install(Arc::clone(&request));
    let caller_scope = ColdComputeCompletenessScope::enter();
    assert!(host.resolve_fallthrough_surface(OWNER).is_some());
    assert!(request.projection_budget.is_exhausted());
    let observed = current_cold_compute_completeness();
    let generic = FallthroughRequestHost::generic_root_propagation(&host);
    for key in [
        fallthrough_cache_key(OWNER, generic, None),
        root_follow_key(OWNER, FallthroughOverrideIdentity::NoOverrides, generic),
    ] {
        assert_eq!(
            candidates(&host, &key),
            0,
            "a resolve-partial surface must stay cold"
        );
    }
    assert!(!mirror_present(&host, OWNER));
    assert_eq!(observed, resolve_completeness);
    caller_scope.discard();
}

/// The MAIN poison guard: a fallthrough compute that consumed a fenced
/// (non-cacheable) serve is SERVED to its caller but must NOT warm the
/// fallthrough node cache.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fenced_serve_fallthrough_node_is_not_admitted() {
    // ── CONTROL: an ORDINARY (cacheable) compute ADMITS. ─────────────────────
    // Anti-vacuity for the fenced arm: it proves the fixture's compute genuinely
    // reaches `store_node` for every key asserted below, so a zero-candidate
    // assertion under the fence is a REFUSAL, not an absent compute.
    let control = build_child_root_host();
    assert!(
        control
            .resolve_fallthrough_surface("/src/Button.vue")
            .is_some(),
        "fixture invariant: the child-root fallthrough resolves",
    );
    for (label, key) in fence_reaching_keys(&control) {
        assert_eq!(
            candidates(&control, &key),
            1,
            "control: an ORDINARY (cacheable) fallthrough compute MUST admit the `{label}` node \
             — otherwise the fenced assertion below is vacuous",
        );
    }

    // ── FENCED: every `ensure_indexed_ready_serve` the compute drives is FENCED
    //    at a stable generation. ───────────────────────────────────────────────
    let host = build_child_root_host();
    let before = node_count(&host);

    let rctx = verter_type_engine::request_context::RequestContext::new(
        1,
        Arc::from("/src/Button.vue"),
        false,
        None,
    );
    let _guard = verter_type_engine::request_context::RequestContextGuard::install(rctx);

    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);
    let resolution = host.resolve_fallthrough_surface("/src/Button.vue");
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    // The value is still SERVED. Cache non-admission is NOT a failed request.
    assert!(
        resolution.is_some(),
        "a fenced fallthrough compute must still SERVE its resolution to the caller — the probe \
         refuses the CACHE WRITE, never the value",
    );

    // Orthogonality: a fenced serve is NON-CACHEABLE, not PARTIAL. This is exactly
    // why the completeness rail cannot catch it, and why the probe rail is the only
    // thing standing between the fence and the cache.
    assert!(
        !verter_type_engine::request_context::current_request_result_is_partial(),
        "a fenced serve is non-cacheable, NOT partial — non-cacheability routes through the fact \
         tracer, never the partial sticky",
    );

    for (label, key) in fence_reaching_keys(&host) {
        assert_eq!(
            candidates(&host, &key),
            0,
            "POISON: a fenced (non-cacheable) fallthrough compute admitted its `{label}` node. A \
             fenced serve is CONTENT-NEUTRAL — the artifact stays published and content-current — \
             so the admitted entry roots on the LIVE hashes and revalidates on every warm read \
             FOREVER. The `CacheabilityProbe` refusal in `store_node` is the ONLY rail that \
             catches it; the cold-compute completeness gate reads `Complete` here and admits.",
        );
    }

    // ── PATH-PRECISION: the rail is not a blanket refusal. ───────────────────
    // Nodes whose OWN compute never touched a serve (their local cacheability
    // scope's probe stayed clean) still admit while the knob is armed. Without
    // this, the fenced arm above could pass by refusing every write
    // unconditionally.
    assert!(
        node_count(&host) > before,
        "the fence must refuse only the nodes whose compute CONSUMED it — a compute whose local \
         scope stayed clean is still cacheable. Zero admissions here would mean the rail is a \
         blanket refusal and the assertions above prove nothing.",
    );
}

/// The SAME fence must also refuse the legacy `cached_fallthrough` MIRROR on
/// `DerivedRawState` — a second admission funnel on the same path, whose
/// `fact_versions` likewise root on the live view.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fenced_serve_cached_fallthrough_mirror_is_not_admitted() {
    // Control — an unfenced compute warms the mirror.
    let control = build_child_root_host();
    assert!(
        !mirror_present(&control, "/src/Button.vue"),
        "fixture invariant: the mirror starts cold",
    );
    assert!(control
        .resolve_fallthrough_surface("/src/Button.vue")
        .is_some());
    assert!(
        mirror_present(&control, "/src/Button.vue"),
        "control: an ORDINARY fallthrough compute MUST warm the `cached_fallthrough` mirror — \
         otherwise the fenced assertion below is vacuous",
    );

    // Fenced — the mirror must stay cold.
    let host = build_child_root_host();
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);
    let resolution = host.resolve_fallthrough_surface("/src/Button.vue");
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    assert!(
        resolution.is_some(),
        "the fenced resolution is still served to the caller",
    );
    assert!(
        !mirror_present(&host, "/src/Button.vue"),
        "POISON: a fenced (non-cacheable) fallthrough compute warmed the legacy \
         `cached_fallthrough` mirror. The mirror's `fact_versions` root on the live view, so a \
         content-neutral fence leaves it validating forever.",
    );
}
