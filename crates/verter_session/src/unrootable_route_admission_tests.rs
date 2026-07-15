//! An UNROOTABLE route walk must mark the enclosing traced compute's rails.
//!
//! The shared-cache funnels ([`crate::resolver_core::route_db::RouteDb`],
//! [`crate::resolver_core::imported_root_db::ImportedRootDb`]) refuse to admit a
//! cold-resolved value unless it carries a non-empty fact signature AND the
//! enclosing cacheability scope is clean. The two refusal reasons are NOT
//! symmetric:
//!
//! - a `probe.non_cacheable()` refusal means the walk consumed a fenced serve /
//!   broken lease / unrootable route — every one of those fanned out to EVERY
//!   tracer on the thread's stack before the funnel ever sampled the probe, so
//!   the enclosing traced compute is already marked;
//! - an `facts.is_empty()` refusal means the RESULT itself is unrootable. No
//!   non-cacheable read need have occurred at all. The enclosing tracer stays
//!   CLEAN and observes NO fact for the route (an empty signature fans nothing).
//!
//! Left unmarked, an enclosing shared-cache entry warm-admits from a compute
//! that consumed a route it cannot root, and revalidates against the live view
//! forever — the route is free to retarget under it with nothing to invalidate
//! on.
//!
//! ## Discrimination contract
//!
//! The fixture drives the REAL producer
//! (`build_named_type_export_route_entry`) to its NORMAL exit with an EMPTY
//! fact vector — no fenced serve, no unresolved wildcard, so neither of the two
//! producer-side marks fires. It reaches that exit through the production host
//! API alone (`upsert` then `evict` — the `did_close` path), with no injected
//! force knob:
//!
//! - the provider's shallow surface still SERVES (the route walk completes and
//!   returns a real `RouteResult`), so the walk is not short-circuited;
//! - `current_or_read_whole_hash` declines for the evicted canonical (the
//!   scheduler branch demands a non-evicted entry; the artifact-only authority
//!   declines for any canonical the scheduler tracks), so no `FileWholeHash`
//!   fact is produced;
//! - the provider has no resolvable surface, so no `Route` fact is produced
//!   either.
//!
//! Both fact arms therefore decline and the producer returns `(route, [])`.
//! Remove the funnel's empty-facts fan-out and `non_cacheable_read_observed`
//! goes back to `false` while the walk still yields its route — which is
//! exactly the poison. The `clean` control below pins the other side: an
//! ordinary rootable route must NOT mark the rails, so the assertion cannot
//! pass by marking everything.

use std::sync::Arc;

use crate::fact_signature_helpers::install_fact_tracer;
use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// A provider whose route walk produces a real result that cannot be rooted:
/// no whole-hash fact (evicted) and no route-surface fact (no resolvable
/// surface). Returns the host, primed and evicted.
fn host_with_unrootable_provider(provider: &str) -> VerterHost {
    let host = host();
    upsert_ts(&host, provider, "");
    // Prime the provider's ARTIFACT while it is still live — not a route. A
    // priming route resolve would admit a `RouteDb` entry under the live
    // provider's facts, and the post-evict lookup would take that warm hit and
    // never reach the cold funnel this test is about.
    let _ = host.ensure_indexed_ready_serve(provider);
    // `did_close`: the canonical stays addressable (its surface still serves)
    // but the whole-hash authority declines for it.
    host.evict(provider);
    host
}

/// Anti-vacuity, on a host of its own.
///
/// The producer must actually REACH its normal exit with an EMPTY fact vector —
/// otherwise the marking assertions below prove nothing. This runs on a
/// dedicated host because the walk is state-changing: it re-materialises the
/// evicted provider's artifact, and the NEXT walk on the same host declines
/// (returns `None`) instead of producing the empty-facts result. Sharing one
/// host with a marking test would consume the exit under test.
#[test]
fn the_route_producer_reaches_its_normal_exit_with_an_empty_fact_signature() {
    let provider = "/ws/anti_vacuity_provider.ts";
    let host = host_with_unrootable_provider(provider);

    let (route, facts) = host
        .build_named_type_export_route_entry(provider, "Missing")
        .expect("the route walk must still produce a result for an evicted provider");

    assert!(
        facts.is_empty(),
        "the fixture no longer reaches the empty-facts producer exit (facts: {facts:?}); \
         the walk produced {route:?}"
    );
    assert!(
        route.is_miss(),
        "the fixture's provider exports nothing, so the walk must MISS — a resolved \
         route would mean the fixture drifted (got {route:?})"
    );
}

#[test]
fn unrootable_route_walk_marks_the_enclosing_traced_compute() {
    let provider = "/ws/unrootable_provider.ts";
    let host = host_with_unrootable_provider(provider);

    // The enclosing traced compute: a shared-cache producer that folds this
    // route into its own result and then decides admission from its rails.
    let (resolved, finalise) = install_fact_tracer(&host, || {
        host.resolve_named_type_export_target(provider, "Missing")
    });
    let non_cacheable = matches!(
        finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    );

    assert!(
        resolved.is_none(),
        "control: the unrootable walk misses, so no target resolves"
    );
    assert!(
        non_cacheable,
        "an enclosing traced compute that consumed an UNROOTABLE (empty-facts) route \
         must observe a non-cacheable read — otherwise it warm-admits a result derived \
         from a route it cannot root, and revalidates against the live view forever"
    );
}

#[test]
fn unrootable_imported_root_walk_marks_the_enclosing_traced_compute() {
    let provider = "/ws/unrootable_root_provider.ts";
    let host = host_with_unrootable_provider(provider);

    let (_root, finalise) = install_fact_tracer(&host, || {
        host.resolve_imported_type_root(provider, "Missing")
    });
    let non_cacheable = matches!(
        finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    );

    assert!(
        non_cacheable,
        "the imported-root funnel shares the same producer and the same empty-facts \
         refusal — its enclosing traced compute must be marked too"
    );
}

#[test]
fn rootable_route_walk_leaves_the_enclosing_traced_compute_clean() {
    let host = host();
    upsert_ts(
        &host,
        "/ws/clean_provider.ts",
        "export type Foo = { a: 1 }\n",
    );

    // The same producer exit, but with a ROOTABLE result: the provider is live,
    // so both fact arms produce. The enclosing compute must stay admissible.
    let (route, facts) = host
        .build_named_type_export_route_entry("/ws/clean_provider.ts", "Foo")
        .expect("route");
    assert!(
        !facts.is_empty(),
        "control fixture must produce a rootable route (got {route:?})"
    );

    let (resolved, finalise) = install_fact_tracer(&host, || {
        host.resolve_named_type_export_target("/ws/clean_provider.ts", "Foo")
    });
    let non_cacheable = matches!(
        finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    );

    assert_eq!(
        resolved,
        Some(("/ws/clean_provider.ts".to_string(), "Foo".to_string())),
        "control: the rootable route resolves"
    );
    assert!(
        !non_cacheable,
        "an ordinary rootable route must NOT mark the rails — otherwise the \
         empty-facts fan-out is indiscriminate and every warm cache dies"
    );
}
