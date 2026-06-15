//! RED test: `RouteDb::get_route_with_facts` returns `(Arc<RouteResult>, Arc<[FactVersionRef]>)`
//! on a warm hit and `None` on a cold miss.

use verter_session::resolver_core::{FactVersionRef, PermissiveStoreView, RouteDb, RouteResult};

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

fn make_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "provider.ts".to_string(),
        defining_symbol: "MyExport".to_string(),
    }
}

fn make_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "provider.ts".to_string(),
        hash: [11u8; 16],
    }
}

#[test]
fn get_route_with_facts_cold_miss_returns_none() {
    let db = RouteDb::new();
    let view = PermissiveStoreView;

    // Nothing inserted — must return None.
    let result = db.get_route_with_facts(&rk("index.ts", "Foo"), &view);
    assert!(result.is_none(), "cold miss must return None");
}

#[test]
fn get_route_with_facts_warm_hit_returns_value_and_facts() {
    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let fact = make_fact();

    // Insert a route with an explicit fact.
    db.insert_route_with_facts(rk("index.ts", "Foo"), make_route(), vec![fact.clone()]);

    // Warm hit must return Some((value, facts)).
    let result = db.get_route_with_facts(&rk("index.ts", "Foo"), &view);
    assert!(result.is_some(), "warm hit must return Some");

    let (route, facts) = result.unwrap();
    assert_eq!(
        *route,
        make_route(),
        "route value must match what was inserted"
    );
    assert_eq!(facts.len(), 1, "must return exactly the one inserted fact");
    assert_eq!(facts[0], fact, "returned fact must match inserted fact");
}

#[test]
fn get_route_with_facts_different_key_still_misses() {
    let db = RouteDb::new();
    let view = PermissiveStoreView;

    db.insert_route_with_facts(rk("a.ts", "Alpha"), make_route(), vec![make_fact()]);

    // Different provider — must miss.
    assert!(
        db.get_route_with_facts(&rk("b.ts", "Alpha"), &view)
            .is_none(),
        "different provider canonical must miss"
    );
    // Same provider, different exported name — must miss.
    assert!(
        db.get_route_with_facts(&rk("a.ts", "Beta"), &view)
            .is_none(),
        "different exported name must miss"
    );
    // Exact match — must hit.
    assert!(
        db.get_route_with_facts(&rk("a.ts", "Alpha"), &view)
            .is_some(),
        "exact match must hit"
    );
}
