//! RED test: `RouteDb::get_or_resolve_route_observing_facts` bubbles the
//! route's fact-dep signature into the active TLS tracer on both warm hits
//! and cold-compute paths.

use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::{
    FactReadSetFinalise, FactVersionRef, PermissiveStoreView, RouteDb, RouteResult,
};
use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

fn route_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "route_dep.ts".to_string(),
        hash: [33u8; 16],
    }
}

fn resolved_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "route_dep.ts".to_string(),
        defining_symbol: "Exported".to_string(),
    }
}

#[test]
fn warm_hit_bubbles_facts_into_active_tracer() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = route_fact();

    // Pre-load the route with a known fact signature.
    db.insert_route_with_facts(
        "index.ts".to_string(),
        "Bar".to_string(),
        resolved_route(),
        vec![fact.clone()],
    );

    // Install a tracer and call the observing variant — the warm hit must bubble
    // the stored fact into our tracer.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        db.get_or_resolve_route_observing_facts("index.ts", "Bar", &view, || {
            unreachable!("resolve closure must not be called on warm hit")
        })
    });

    assert!(result.is_some(), "warm hit must return Some");
    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "tracer must contain the route's fact after warm hit; got {sig:?}"
            );
        }
        FactReadSetFinalise::Overflow => panic!("tracer overflowed"),
    }
}

#[test]
fn cold_compute_bubbles_facts_after_resolve() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = route_fact();
    let fact_for_closure = fact.clone();

    // Nothing pre-loaded — the resolve closure runs and returns facts.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        db.get_or_resolve_route_observing_facts("index.ts", "Baz", &view, move || {
            Some((resolved_route(), vec![fact_for_closure.clone()]))
        })
    });

    assert!(result.is_some(), "cold compute must return Some");
    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "tracer must contain the route's fact after cold compute; got {sig:?}"
            );
        }
        FactReadSetFinalise::Overflow => panic!("tracer overflowed"),
    }
}

#[test]
fn cold_miss_returns_none_and_tracer_empty() {
    let host = make_host();
    let db = RouteDb::new();
    let view = PermissiveStoreView;

    // Resolve closure returns None — the route is unresolvable.
    let (result, finalise) = install_fact_tracer_for_tests(&host, || {
        db.get_or_resolve_route_observing_facts("index.ts", "Unknown", &view, || None)
    });

    assert!(result.is_none(), "unresolvable route must return None");
    // On miss the tracer must be empty (no facts to bubble).
    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.is_empty(),
                "tracer must be empty on unresolved route; got {sig:?}"
            );
        }
        FactReadSetFinalise::Overflow => panic!("tracer overflowed on empty path"),
    }
}

#[test]
fn no_active_tracer_warm_hit_still_returns_value() {
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = route_fact();

    db.insert_route_with_facts(
        "index.ts".to_string(),
        "Qux".to_string(),
        resolved_route(),
        vec![fact],
    );

    // No tracer installed — observe_fact_signature must be a no-op, but the
    // route must still be returned.
    let result = db.get_or_resolve_route_observing_facts("index.ts", "Qux", &view, || {
        unreachable!("resolve closure must not be called on warm hit")
    });
    assert!(
        result.is_some(),
        "must return route value even with no tracer"
    );
}
