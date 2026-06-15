//! RED test (closed by same-block implementation) — NEGATIVE
//! discriminator: editing an unrelated route (different provider)
//! does NOT invalidate an already-warm route's cached fact-dep
//! signature. The second consumer call on the original route hits
//! the warm branch, bubbles the cached facts, and advances
//! `route_warm_fact_bubble_emissions` — it MUST NOT fall through
//! to the cold counter.
//!
//! ## Discrimination contract
//!
//! This test FAILS unless ALL of the following hold:
//!
//! 1. Eviction of a foreign provider via `evict_provider` does NOT
//!    drop the original provider's cache slot.
//! 2. The second `get_or_resolve_route_observing_facts` call on the
//!    original route returns through the warm-hit branch (advances
//!    `route_warm_fact_bubble_emissions` by exactly one).
//! 3. The cold-resolve and coalesced-join counters DO NOT advance
//!    between the two snapshots — over-invalidation that drops the
//!    original entry would force a cold re-resolve and advance
//!    `route_cold_fact_bubble_emissions` instead.
//!
//! ## Why this could not be expressed before the consumer migration
//!
//! Before the consumer migration, the observing entry-point did not
//! expose a per-branch counter set. A regression that over-evicted
//! (drop original on foreign edit) would silently fall back to the
//! cold path, but tests could not discriminate that fall-back from a
//! genuine warm hit because the bubble emission was not counted per
//! branch. This test snapshots all three counters, mutates an
//! unrelated route surface via `evict_provider`, calls observing on
//! the original route, and asserts exactly one warm-counter
//! advancement with the cold and coalesced counters flat.

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

/// The fact signature carried by the original (preserved) route.
/// Discrimination relies on this exact ref re-appearing in the
/// post-edit tracer scope.
fn original_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "orig_dep.ts".to_string(),
        hash: [0x44u8; 16],
    }
}

fn original_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "orig_dep.ts".to_string(),
        defining_symbol: "OrigExport".to_string(),
    }
}

/// The fact signature carried by the unrelated (foreign) route that
/// will be evicted. The original route's `fact_dep_signature` does
/// NOT reference any of these bytes — proving the eviction cannot
/// invalidate the original entry by fact-signature membership.
fn foreign_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "foreign_dep.ts".to_string(),
        hash: [0x55u8; 16],
    }
}

fn foreign_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "foreign_dep.ts".to_string(),
        defining_symbol: "ForeignExport".to_string(),
    }
}

#[test]
fn unrelated_route_eviction_keeps_original_warm() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;

    // Prime two unrelated routes under different providers. The
    // original route is what we care about; the foreign route
    // exists only so we can evict it and observe that the eviction
    // leaves the original untouched.
    db.insert_route_with_facts(
        rk("orig_provider.ts", "OrigName"),
        original_route(),
        vec![original_fact()],
    );
    db.insert_route_with_facts(
        rk("foreign_provider.ts", "ForeignName"),
        foreign_route(),
        vec![foreign_fact()],
    );

    // First observing call — warm hit on the original. Confirms the
    // priming and locks in the baseline counter values for the
    // second-call delta.
    let _ = install_fact_tracer_for_tests(&host, || {
        db.get_or_resolve_route_observing_facts(rk("orig_provider.ts", "OrigName"), &view, || {
            unreachable!("first call: original route is pre-warmed")
        })
    });

    let warm_baseline = db.route_warm_fact_bubble_emissions();
    let cold_baseline = db.route_cold_fact_bubble_emissions();
    let coalesced_baseline = db.route_coalesced_fact_bubble_emissions();
    assert!(
        warm_baseline >= 1,
        "first call must have advanced the warm counter at least once"
    );

    // Mutate an unrelated route surface — evict the foreign
    // provider's route slot. Under correct fact-validation
    // semantics this MUST NOT drop the original provider's entry.
    db.evict_provider("foreign_provider.ts");

    // Second observing call on the ORIGINAL route. Must hit the
    // warm branch (the original entry was preserved across the
    // foreign eviction), advance the warm counter by exactly one,
    // and leave both the cold and coalesced counters flat. A
    // regression that over-evicted would force a cold re-resolve
    // here — the closure would run, the cold counter would advance,
    // and the `unreachable!` below would trip.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        db.get_or_resolve_route_observing_facts(rk("orig_provider.ts", "OrigName"), &view, || {
            unreachable!(
                "second call: original route must STILL be warm after \
                 foreign-route eviction — closure must not run"
            )
        })
    });
    assert!(
        result.is_some(),
        "warm hit after foreign eviction returns Some"
    );

    let warm_after = db.route_warm_fact_bubble_emissions();
    let cold_after = db.route_cold_fact_bubble_emissions();
    let coalesced_after = db.route_coalesced_fact_bubble_emissions();

    assert_eq!(
        warm_after.saturating_sub(warm_baseline),
        1,
        "second call (after foreign eviction) MUST hit the warm \
         branch and advance `route_warm_fact_bubble_emissions` by \
         exactly one. warm_baseline={warm_baseline} \
         warm_after={warm_after}. If this fails, the foreign \
         eviction over-invalidated the original entry — fact \
         validation is wrongly considering the original entry stale \
         on a foreign-only edit."
    );
    assert_eq!(
        cold_after, cold_baseline,
        "second call MUST NOT advance the cold counter — a cold \
         advancement here means the original entry was dropped by \
         the foreign eviction and re-resolved through the cold path. \
         cold_baseline={cold_baseline} cold_after={cold_after}."
    );
    assert_eq!(
        coalesced_after, coalesced_baseline,
        "second call MUST NOT advance the coalesced counter — \
         single-threaded warm hit only. \
         coalesced_baseline={coalesced_baseline} \
         coalesced_after={coalesced_after}."
    );

    // The original fact MUST still bubble after the foreign
    // eviction — proving the fact signature is intact and the
    // tracer captures the original entry's deps.
    let want = original_fact();
    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &want),
                "warm hit after foreign eviction MUST still bubble \
                 the original route's fact-dep signature (expected \
                 to contain {want:?}; got {sig:?})."
            );
            // Cross-check: the foreign fact MUST NOT appear in the
            // original's tracer — confirms the eviction did not
            // leak foreign signature bytes into the original entry.
            let foreign = foreign_fact();
            assert!(
                !sig.iter().any(|f| f == &foreign),
                "foreign fact must NOT appear in the original \
                 route's bubbled signature (foreign={foreign:?}; \
                 got {sig:?})."
            );
        }
        FactReadSetFinalise::Overflow => panic!("warm-after-foreign-evict tracer overflowed"),
    }
}
