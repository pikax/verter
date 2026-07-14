//! RED test (closed by same-block implementation): the consumer-facing
//! `RouteDb::get_or_resolve_route_observing_facts` cold-resolve branch
//! advances `route_cold_fact_bubble_emissions` AND bubbles the
//! freshly-admitted route's `fact_dep_signature` into the current
//! thread's active tracer via a post-admission re-read.
//!
//! ## Discrimination contract
//!
//! This test FAILS unless ALL of the following are wired end-to-end:
//!
//! 1. The cold-resolve branch (singleflight leader role) runs the
//!    inner `resolve` closure, admits the result into the validated
//!    cache with the closure-supplied fact-dep signature, then
//!    re-reads the admitted entry through `get_route_with_facts`
//!    and fans the facts into the current thread's active tracer.
//! 2. The same branch bumps `route_cold_fact_bubble_emissions` once
//!    on the leader-role re-read.
//!
//! Reverting site (1) makes the tracer assertion fail (the tracer
//! observes an empty set even though the closure returned facts).
//! Reverting site (2) makes the counter-delta assertion fail. The
//! `unreachable!` in the parallel test on the warm branch is a
//! redundant safety check for this test — the resolve closure here
//! MUST run, and the test asserts it ran exactly once.
//!
//! ## Why this could not be expressed before the consumer migration
//!
//! Before the consumer migration, the cold-resolve path returned the
//! result Arc without a post-admission re-read. The closure's
//! signature was admitted into the cache but never fanned into the
//! caller's tracer scope on the cold call — facts from a cold
//! resolve only reached subsequent warm-hit callers. This test
//! installs the tracer, calls observing-facts on a cold key, and
//! asserts BOTH that the tracer captures the closure's signature AND
//! that the cold counter advances.

use std::sync::atomic::{AtomicU32, Ordering};

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

/// `FactVersionRef` with a recognisable 16-byte pattern. The pattern
/// is what the closure stores via `insert_arc_with_kind` inside the
/// singleflight, and what the re-read returns. The tracer assertion
/// below matches against this exact ref.
fn route_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "cold_dep.ts".to_string(),
        hash: [0x22u8; 16],
    }
}

fn resolved_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "cold_dep.ts".to_string(),
        defining_symbol: "ColdExport".to_string(),
    }
}

#[test]
fn cold_resolve_advances_cold_counter_and_bubbles_admitted_facts() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = route_fact();
    let fact_for_closure = fact.clone();

    let calls = AtomicU32::new(0);

    // Snapshot all three counters before the cold call.
    let warm_before = db.route_warm_fact_bubble_emissions();
    let cold_before = db.route_cold_fact_bubble_emissions();
    let coalesced_before = db.route_coalesced_fact_bubble_emissions();

    // Cold call — the route is absent from the cache, the resolve
    // closure runs and returns a non-empty fact signature. The
    // observing entry-point admits the entry, re-reads, and bubbles.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::with_cacheability_scope_for_tests(&host, |probe| {
            db.get_or_resolve_route_observing_facts(
                rk("cold_provider.ts", "Baz"),
                &view,
                probe,
                || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Some((resolved_route(), vec![fact_for_closure.clone()]))
                },
            )
        })
        .0
    });
    assert!(result.is_some(), "cold resolve must return Some");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "cold resolve must invoke the closure exactly once"
    );

    let warm_after = db.route_warm_fact_bubble_emissions();
    let cold_after = db.route_cold_fact_bubble_emissions();
    let coalesced_after = db.route_coalesced_fact_bubble_emissions();

    assert_eq!(
        cold_after.saturating_sub(cold_before),
        1,
        "cold-resolve branch MUST advance \
         `route_cold_fact_bubble_emissions` by exactly one. \
         cold_before={cold_before} cold_after={cold_after}. If this \
         fails, the leader-role re-read is not bumping the cold \
         counter — the post-admission fact bubble is missing."
    );
    assert_eq!(
        warm_after, warm_before,
        "cold-resolve branch MUST NOT advance the warm counter. \
         warm_before={warm_before} warm_after={warm_after}."
    );
    assert_eq!(
        coalesced_after, coalesced_before,
        "single-thread cold-resolve branch MUST NOT advance the \
         coalesced-join counter. coalesced_before={coalesced_before} \
         coalesced_after={coalesced_after}."
    );

    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "cold resolve MUST bubble the freshly-admitted route's \
                 fact-dep signature into the active tracer via a \
                 post-admission re-read (expected to contain {fact:?}; \
                 got {sig:?}). If this fails, the cold-resolve branch \
                 returned the route Arc without re-reading and fanning \
                 facts into the tracer scope."
            );
        }
        FactReadSetFinalise::Overflow => panic!("cold-resolve tracer overflowed"),
    }
}
