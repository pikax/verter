//! RED test (closed by same-block implementation): the consumer-facing
//! `RouteDb::get_or_resolve_route_observing_facts` warm-hit branch
//! advances `route_warm_fact_bubble_emissions` AND bubbles the cached
//! route's `fact_dep_signature` into the current thread's active
//! tracer.
//!
//! ## Discrimination contract
//!
//! This test FAILS unless ALL of the following are wired end-to-end:
//!
//! 1. The warm-hit fast-path calls `observe_fact_signature(&facts)`
//!    on the cached entry's stored fact-dep signature BEFORE returning.
//! 2. The same warm-hit branch bumps
//!    `route_warm_fact_bubble_emissions` once per warm hit.
//!
//! Reverting either site makes one of the two assertions below fail
//! independently — the tracer assertion catches site (1), and the
//! counter-delta assertion catches site (2).
//!
//! ## Why this could not be expressed before the consumer migration
//!
//! Before the consumer migration onto
//! `get_or_resolve_route_observing_facts`, consumer call sites used
//! `get_route` / `get_or_resolve_route_with_facts` directly — neither
//! installed any fact-bubbling on the warm-hit path. A warm consumer
//! call observed an empty tracer even when the cached route entry
//! carried a non-empty signature. This test snapshots the warm
//! counter, calls the observing entry-point on a pre-warmed entry,
//! and then asserts that BOTH the counter advances AND the tracer
//! contains the cached fact.

use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::{
    FactReadSetFinalise, FactVersionRef, PermissiveStoreView, RouteDb, RouteResult,
};
use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

fn rk(provider: &str, name: &str) -> verter_session::resolver_core::RouteNameKey {
    verter_session::resolver_core::RouteNameKey::new(
        provider,
        name,
        verter_semantic::facts::registry::SymbolSpace::Type,
        verter_session::file_artifact_store::ProjectIdentity([0u8; 16]),
        [0u8; 16],
        [0u8; 16],
    )
}

/// Build a `FactVersionRef` with a recognisable 16-byte pattern. The
/// pattern reappears in the joiner's outer tracer when the bubble
/// path fires, so the discrimination assertion can match the fact
/// shape directly.
fn route_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "warm_dep.ts".to_string(),
        hash: [0x11u8; 16],
    }
}

fn resolved_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "warm_dep.ts".to_string(),
        defining_symbol: "WarmExport".to_string(),
    }
}

#[test]
fn warm_hit_advances_warm_counter_and_bubbles_route_facts() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = route_fact();

    // Pre-load the route entry with a known fact signature. The
    // signature must be non-empty for strict admission to succeed
    // and for the warm-hit branch to bubble a non-empty fact set
    // into the tracer.
    db.insert_route_with_facts(
        rk("warm_provider.ts", "Bar"),
        resolved_route(),
        vec![fact.clone()],
    );

    // Snapshot the warm counter before the observing call.
    let warm_before = db.route_warm_fact_bubble_emissions();
    let cold_before = db.route_cold_fact_bubble_emissions();
    let coalesced_before = db.route_coalesced_fact_bubble_emissions();

    // Install a tracer and call the observing entry-point. The warm
    // branch MUST bubble the cached fact into the tracer AND advance
    // the warm counter by exactly one.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::with_cacheability_scope_for_tests(&host, |_probe| {
            db.get_or_resolve_route_observing_facts(
                rk("warm_provider.ts", "Bar"),
                &view,
                &host,
                || unreachable!("resolve closure must not run on warm hit"),
            )
        })
        .0
    });
    assert!(result.is_some(), "warm hit must return Some");

    let warm_after = db.route_warm_fact_bubble_emissions();
    let cold_after = db.route_cold_fact_bubble_emissions();
    let coalesced_after = db.route_coalesced_fact_bubble_emissions();

    assert_eq!(
        warm_after.saturating_sub(warm_before),
        1,
        "warm-hit branch MUST advance `route_warm_fact_bubble_emissions` \
         by exactly one per warm consumer call. warm_before={warm_before} \
         warm_after={warm_after}. If this fails, the warm-hit branch \
         does not bump the warm counter — the consumer migration onto \
         `get_or_resolve_route_observing_facts` is incomplete."
    );
    assert_eq!(
        cold_after, cold_before,
        "warm-hit branch MUST NOT advance the cold-resolve counter. \
         cold_before={cold_before} cold_after={cold_after}. If this fails, \
         the warm branch is mis-routing through the cold path."
    );
    assert_eq!(
        coalesced_after, coalesced_before,
        "warm-hit branch MUST NOT advance the coalesced-join counter. \
         coalesced_before={coalesced_before} \
         coalesced_after={coalesced_after}."
    );

    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "warm hit MUST bubble the cached route's \
                 `fact_dep_signature` into the active tracer (expected \
                 to contain {fact:?}; got {sig:?}). If this fails, the \
                 warm-hit branch is not calling `observe_fact_signature` \
                 on the cached signature."
            );
        }
        FactReadSetFinalise::NonCacheable(_) => {
            panic!("warm-hit tracer unexpectedly non-cacheable")
        }
        FactReadSetFinalise::Overflow => panic!("warm-hit tracer overflowed"),
    }
}
