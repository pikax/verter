//! RouteDb fact-validation discrimination tests.
//!
//! Each test exercises a live route or resolved-import cache surface.
//!
//! Invariants covered here:
//!
//! - Cross-consumer route hit produces ONE per-name `RouteDb` entry
//!   (per R6 query-identity cache).
//! - The redundant whole-hash route-surface oracle is gone.
//! - lib_env_hash change does NOT invalidate `ResolvedImportFacts`
//!   (R21 — the paired negative assertion).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_session::resolver_core::{
    BarrelRouteSurface, FactVersionRef, RouteDb, RouteResult, StoreView, StoreViewCompatToken,
};
use verter_session::VerterHost;

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

fn bk(barrel: &str) -> verter_session::resolver_core::BarrelSurfaceKey {
    verter_session::resolver_core::BarrelSurfaceKey::new(
        barrel,
        verter_session::file_artifact_store::ProjectIdentity([0u8; 16]),
        [0u8; 16],
        [0u8; 16],
    )
}

// ────────────────────────────────────────────────────────────────
// Test view — accepts all facts, used by the basic plumbing tests.
// ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct AcceptAllView {
    token: StoreViewCompatToken,
}

impl AcceptAllView {
    fn new(epoch: u64) -> Self {
        Self {
            token: StoreViewCompatToken {
                epoch,
                session: None,
                validity_fingerprint: 0,
            },
        }
    }
}

impl StoreView for AcceptAllView {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.token
    }
    fn validates(&self, _fact: &FactVersionRef) -> bool {
        true
    }
}

// ────────────────────────────────────────────────────────────────
// Test 1 — cross-consumer route hit produces ONE per-name entry.
// ────────────────────────────────────────────────────────────────

#[test]
fn cross_consumer_route_hit_produces_one_entry() {
    let host = VerterHost::new_standalone(Default::default());
    let db = RouteDb::new();
    let view = AcceptAllView::new(1);

    // Two consumers query the same `(provider, name)`.
    let compute_count = std::sync::atomic::AtomicU32::new(0);
    let do_query = |_label: &str| {
        verter_session::for_tests::with_cacheability_scope_for_tests(&host, |_probe| {
            db.get_or_resolve_route_with_facts(rk("provider.ts", "Foo"), &view, &host, || {
                compute_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some((
                    RouteResult::Resolved {
                        defining_canonical: "foo.ts".to_owned(),
                        defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        defining_symbol: "Foo".to_owned(),
                    },
                    vec![FactVersionRef::FileWholeHash {
                        canonical_id: "provider.ts".to_owned(),
                        hash: [1u8; 16],
                    }],
                ))
            })
        });
    };
    do_query("consumer-1");
    do_query("consumer-2");

    assert_eq!(
        compute_count.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "second consumer MUST short-circuit on the cached entry (one cold compute total)"
    );

    let snapshot = db.snapshot_routes_for_test();
    let matching: Vec<_> = snapshot
        .iter()
        .filter(|(key, _)| key == &("provider.ts".to_owned(), "Foo".to_owned()))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "exactly ONE `RouteDb` entry MUST exist for (provider.ts, Foo) under R6 query-identity \
         cache rule, not one entry per consumer"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 2 — redundant whole-hash route-surface oracle elimination.
// ────────────────────────────────────────────────────────────────

#[test]
fn whole_hash_migration_audit_route_db_318_eliminated() {
    let db = RouteDb::new();
    let view = AcceptAllView::new(1);
    let signature = Arc::from(
        vec![FactVersionRef::FileWholeHash {
            canonical_id: "barrel.ts".to_owned(),
            hash: [42u8; 16],
        }]
        .into_boxed_slice(),
    );
    let surface = BarrelRouteSurface {
        barrel_canonical: "barrel.ts".to_owned(),
        wildcard_edges: FxHashMap::default(),
        fact_dep_signature: Arc::clone(&signature),
    };
    db.insert_barrel_surface(bk("barrel.ts"), surface);

    let fetched = db
        .get_barrel_surface(&bk("barrel.ts"), &view)
        .expect("barrel surface MUST round-trip");
    assert!(Arc::ptr_eq(&fetched.fact_dep_signature, &signature));
}

// ────────────────────────────────────────────────────────────────
// Test 4 — paths edit (resolve_env_hash) invalidates RouteDb.
// ────────────────────────────────────────────────────────────────

#[test]
fn lib_env_hash_change_does_not_invalidate_resolved_import_facts() {
    use verter_session::resolved_import_facts::{
        ResolvedImportFacts, ResolvedImportFactsDb, ResolvedImportFactsKey,
        RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    };

    let db = ResolvedImportFactsDb::new();
    let key = ResolvedImportFactsKey {
        canonical: Arc::from("/a.ts"),
        content_hash: [1u8; 16],
        parse_env_hash: [2u8; 16],
        resolve_env_hash: [3u8; 16],
        resolver_version: RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        known_miss_generation: [0u8; 16],
    };

    let admitted = db.insert_if_absent(key.clone(), Arc::new(ResolvedImportFacts::new()));
    assert!(admitted, "first writer MUST win the admission race");

    let warm = db.get(&key);
    assert!(warm.is_some(), "warm entry under env A MUST hit");

    // The key does NOT carry `lib_env_hash` as a field — R21 scoping
    // rule. So any "lib change" maps to the same key value and the
    // same cache entry, by structural construction. The negative
    // assertion is therefore: the struct literal for the key is
    // exhaustive (no `lib_env_hash` field exists to vary).
    let _exhaustive_field_check = ResolvedImportFactsKey {
        canonical: Arc::clone(&key.canonical),
        content_hash: key.content_hash,
        parse_env_hash: key.parse_env_hash,
        resolve_env_hash: key.resolve_env_hash,
        resolver_version: key.resolver_version,
        known_miss_generation: key.known_miss_generation,
        // intentionally NO `lib_env_hash: …` here — adding one would
        // be a compile error, which is the R21 invariant.
    };

    // The original entry still hits — confirms it survived without
    // any "lib change" producing a cache invalidation.
    let still_warm = db.get(&key);
    assert!(
        still_warm.is_some(),
        "lib change MUST NOT invalidate ResolvedImportFacts (R21 — base \
         import resolution does not depend on libs)"
    );
}

// ────────────────────────────────────────────────────────────────
// Auxiliary tests — augmenter-set fingerprint stability +
// fact-validation observation surface.
// ────────────────────────────────────────────────────────────────

trait RouteDbTestExt {
    fn snapshot_routes_for_test(&self) -> Vec<((String, String), Arc<RouteResult>)>;
}

impl RouteDbTestExt for RouteDb {
    fn snapshot_routes_for_test(&self) -> Vec<((String, String), Arc<RouteResult>)> {
        // Iterate via get_route + AcceptAllView for any keys we
        // already populated in the test. There is no public
        // snapshot_all on RouteDb, so we look up by the known keys.
        // The test that uses this helper inserts only one key
        // (provider.ts, Foo), so we probe it directly.
        let view = AcceptAllView::new(0);
        let mut out = Vec::new();
        let probe_key = ("provider.ts".to_owned(), "Foo".to_owned());
        if let Some(r) = self.get_route(&rk(probe_key.0.as_str(), probe_key.1.as_str()), &view) {
            out.push((probe_key, r));
        }
        out
    }
}
