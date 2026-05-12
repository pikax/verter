//! Stage-5 landing-gap A discriminating production-flow tests.
//!
//! Binds **R17** (sessions are views; query paths never call host.upsert)
//! and **R18** (SessionView is passed explicitly through ResolverContext
//! — no thread-local view globals). These tests exercise the wiring of
//! `SessionView` through `MetaSession::get_component_meta`: the
//! consumer path must consult `view.content_hash_for(canonical)` when
//! deriving the cache key, NOT the base host's `shallow.whole_hash`.
//!
//! Discriminating contract:
//!
//! - Pre-wiring: `MetaSession::get_component_meta` reads the base host's
//!   content hash regardless of the session's overlay, so a session with
//!   an overlay whose hash differs from the base would still hit the
//!   base-keyed cache slot. The `view_aware_cache_key_lookups` counter
//!   is invariant — no wiring exists to bump it.
//! - Post-wiring: the session constructs an `OverlaidView` and the
//!   host's consumer path reads the view's hash. The
//!   `view_aware_cache_key_lookups` counter increments per session
//!   query, the cache key for the second read (with overlay) differs
//!   from the base-warmed slot, and `host.upsert` is never called.
//!
//! Each test is structured to FAIL pre-fix and PASS post-fix without
//! relying on key-shape `HashSet` checks or compile-time signal alone.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};

fn sfc(props: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {props} }}>()
</script>
<template><div>hello</div></template>"#
    )
}

fn fresh_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn host_upsert_count(project: &Arc<MetaProject>) -> u64 {
    project
        .host()
        .provenance()
        .host_upsert_calls
        .load(Ordering::Acquire)
}

fn view_aware_lookup_count(project: &Arc<MetaProject>) -> u64 {
    project
        .host()
        .provenance()
        .view_aware_cache_key_lookups
        .load(Ordering::Acquire)
}

fn component_meta_entry_count(project: &Arc<MetaProject>) -> usize {
    project
        .host()
        .project_type_store()
        .component_meta_results()
        .len()
}

/// **Gap A primary test** — proves the consumer path is routed through
/// `SessionView::content_hash_for(canonical)` when deriving the cache key.
///
/// Discrimination strategy:
///
/// 1. Warm the base host's component-meta cache for `/Comp.vue` by
///    calling `host.get_component_meta(...)` directly. This populates
///    a cache entry keyed on the base content hash `H_base`.
/// 2. Open a `MetaSession` and install an overlay with DIFFERENT
///    content (different prop set) — hash `H_overlay != H_base`.
/// 3. Query through the session. The session MUST consult its
///    `OverlaidView` for the content hash; pre-fix it reads the base
///    hash and incorrectly hits the warm slot (causing a wrong
///    overlay semantics, but more importantly failing to bump
///    `view_aware_cache_key_lookups`). Post-fix the counter increments
///    AND the second cache entry is keyed under the overlay hash.
/// 4. R17 invariant: `host.upsert` is invariant under all session
///    queries.
#[test]
fn metasession_query_routes_through_session_view_content_hash() {
    let project = fresh_project();
    project
        .upsert_base("/Comp.vue", &sfc("msg: string"))
        .expect("base upsert");

    // 1. Warm the base host's component-meta cache.
    let _ = project
        .host()
        .get_component_meta("/Comp.vue")
        .expect("base host warm path returns Some");

    let entry_count_after_base_warm = component_meta_entry_count(&project);
    let host_upserts_before = host_upsert_count(&project);
    let view_aware_before = view_aware_lookup_count(&project);

    assert!(
        entry_count_after_base_warm >= 1,
        "control: warming via host.get_component_meta must produce >=1 cache entry, got {entry_count_after_base_warm}"
    );

    // 2. Open a session and install an overlay with DIFFERENT
    //    content (different prop set → different content hash).
    let session = project.open_session().expect("session");
    session
        .upsert("/Comp.vue", sfc("count: number"))
        .expect("session overlay upsert");

    let host_upserts_after_session_upsert = host_upsert_count(&project);
    assert_eq!(
        host_upserts_before, host_upserts_after_session_upsert,
        "R17: session.upsert MUST NOT call host.upsert (overlay is session-local). \
         Counter moved from {host_upserts_before} → {host_upserts_after_session_upsert}."
    );

    // 3. Query through the session — the consumer path must read the
    //    overlay's content hash via `SessionView::content_hash_for`.
    let _ = session.get_component_meta("/Comp.vue");

    let view_aware_after_first_session_query = view_aware_lookup_count(&project);
    let host_upserts_after_first_query = host_upsert_count(&project);

    // Pre-fix discrimination: `view_aware_cache_key_lookups` is invariant
    // because nothing increments it (no wiring exists). Post-fix the
    // session's consumer path bumps the counter exactly once per query.
    assert!(
        view_aware_after_first_session_query > view_aware_before,
        "POST-WIRE: session consumer path MUST increment view_aware_cache_key_lookups when \
         reading view.content_hash_for(canonical). Counter invariant from {view_aware_before} \
         to {view_aware_after_first_session_query} indicates the wire is not in place."
    );

    // R17 invariant — the query path never calls host.upsert.
    assert_eq!(
        host_upserts_before, host_upserts_after_first_query,
        "R17: session.get_component_meta MUST NOT call host.upsert. \
         Counter moved from {host_upserts_before} → {host_upserts_after_first_query}."
    );

    // 4. Second session query — the view-aware counter increments again
    //    (per-query observable wiring), proving the consumer reads the
    //    view on every call, not just once.
    let _ = session.get_component_meta("/Comp.vue");
    let view_aware_after_second = view_aware_lookup_count(&project);
    assert!(
        view_aware_after_second > view_aware_after_first_session_query,
        "POST-WIRE: each session query must consult the view's content_hash_for. \
         Counter held at {view_aware_after_first_session_query} on the second query."
    );
}

/// **Gap A — two-session isolation.** Two concurrent sessions with
/// conflicting overlays on the same canonical produce DIFFERENT
/// view-derived cache keys, so the `ComponentMetaResultDb` accumulates
/// separate entries per overlay hash.
///
/// Discrimination strategy:
///
/// 1. Base content hash H_base; warm via `host.get_component_meta`.
/// 2. Session A overlay hash H_A; query through session A.
/// 3. Session B overlay hash H_B; query through session B.
/// 4. Assert the cache has accumulated MORE entries than the base
///    alone — proving the consumer paths used view-derived hashes
///    instead of collapsing onto the base slot.
/// 5. R17 invariant — host.upsert is invariant under all session
///    operations.
#[test]
fn two_concurrent_sessions_with_conflicting_overlays_isolated_in_cache() {
    let project = fresh_project();
    project
        .upsert_base("/Comp.vue", &sfc("base: string"))
        .expect("base upsert");

    // Warm the base.
    let _ = project.host().get_component_meta("/Comp.vue");
    let base_entry_count = component_meta_entry_count(&project);
    let host_upserts_before = host_upsert_count(&project);

    // Two sessions with DISTINCT overlay content.
    let session_a = project.open_session().expect("session A");
    let session_b = project.open_session().expect("session B");

    session_a
        .upsert("/Comp.vue", sfc("overlay_a: string"))
        .expect("session A overlay");
    session_b
        .upsert("/Comp.vue", sfc("overlay_b: string"))
        .expect("session B overlay");

    let host_upserts_after_session_upserts = host_upsert_count(&project);
    assert_eq!(
        host_upserts_before, host_upserts_after_session_upserts,
        "R17: session upserts MUST NOT touch host.upsert"
    );

    // Query both sessions.
    let _ = session_a.get_component_meta("/Comp.vue");
    let _ = session_b.get_component_meta("/Comp.vue");

    let host_upserts_after_queries = host_upsert_count(&project);
    assert_eq!(
        host_upserts_before, host_upserts_after_queries,
        "R17: session queries MUST NOT call host.upsert. \
         Counter moved {host_upserts_before} → {host_upserts_after_queries}."
    );

    // Each session's view yields a distinct content hash. The consumer
    // path keyed on view-derived hashes admits a fresh cache slot per
    // overlay hash, so the entry count is strictly greater than the
    // base-only warm.
    let final_entry_count = component_meta_entry_count(&project);
    assert!(
        final_entry_count > base_entry_count,
        "POST-WIRE: two sessions with conflicting overlays MUST produce distinct \
         view-derived cache keys, growing the cache beyond the base. \
         base={base_entry_count}, final={final_entry_count}. \
         A non-increasing count indicates the consumer path collapsed onto the \
         base hash (the view was not consulted)."
    );
}

/// **Gap A — session teardown does not mutate base.** Confirms R17:
/// after `drop(session)`, the base host's view of the canonical is
/// unchanged.
#[test]
fn session_drop_does_not_affect_base_cache() {
    let project = fresh_project();
    project
        .upsert_base("/Comp.vue", &sfc("base: string"))
        .expect("base upsert");

    // Warm the base.
    let _ = project.host().get_component_meta("/Comp.vue");

    let host_upserts_before = host_upsert_count(&project);

    // Open a session, install an overlay, query, drop.
    {
        let session = project.open_session().expect("session");
        session
            .upsert("/Comp.vue", sfc("overlay: number"))
            .expect("session overlay");
        let _ = session.get_component_meta("/Comp.vue");
        drop(session);
    }

    let host_upserts_after_drop = host_upsert_count(&project);
    assert_eq!(
        host_upserts_before, host_upserts_after_drop,
        "R17: session lifecycle MUST NOT call host.upsert. \
         Counter moved {host_upserts_before} → {host_upserts_after_drop}."
    );

    // The base host's own query is still observable and stable —
    // the dropped session's overlay did not corrupt the base cache.
    let base_meta_after = project
        .host()
        .get_component_meta("/Comp.vue")
        .expect("base host meta after session drop");

    // Validate it still reflects the base content — the base prop is
    // `base: string`, not `overlay: number`.
    let base_prop_names: Vec<String> = base_meta_after
        .props
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert!(
        base_prop_names.iter().any(|n| n == "base"),
        "base host's meta after session drop MUST still reflect base props, got: {base_prop_names:?}"
    );
}

// `OverlaidView` direct substrate surface is covered by
// `tests/session_view_smoke.rs` — see
// `overlaid_view_byte_identical_overlay_matches_base_hash` and
// `overlaid_view_diverging_overlay_diverges_in_hash`. The tests in
// this file exercise the production-flow consumer path that consults
// the view; the substrate behavior is not retested here.
