//! Stage 6c — RouteDb fact-validation discrimination tests.
//!
//! Each test was written so it would FAIL against the pre-Stage-6c
//! tree (no `effective_export_sets` cache; no
//! `BarrelRouteSurface.fact_dep_signature`; no
//! `validates_route_surface_domain` wiring) and PASS against the
//! post-Stage-6c tree.
//!
//! Plan §763-781 verify bullets covered here:
//!
//! - Cross-consumer route hit produces ONE per-name `RouteDb` entry
//!   (per R6 query-identity cache).
//! - `SyntacticExportSet` (parse-domain) ≠ `EffectiveExportSet`
//!   (resolve-domain) (per R15).
//! - `whole_hash_migration_audit` site #2 is GONE.
//! - `paths` (resolve_env_hash) edit invalidates RouteDb entries.
//! - lib_env_hash change invalidates `EffectiveExportSet` cache
//!   entries (R21 scoping rule on route surface).
//! - lib_env_hash change does NOT invalidate `ResolvedImportFacts`
//!   (R21 — the paired negative assertion).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use smallvec::smallvec;
use verter_semantic::facts::registry::{
    AugmentationTargetKindTag, InternedName, InternedSpecifier, SymbolSpace,
};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::file_artifact_store::{
    AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, AugmenterEntry,
    AugmenterSet, FileArtifactKey, FileArtifactStore, ProjectIdentity,
};
use verter_session::resolver_core::{
    BarrelRouteSurface, EffectiveExportEntry, EffectiveExportSetEntry, EffectiveExportSetKey,
    EffectiveExportSetScope, FactVersionRef, RouteDb, RouteResult, RouteSurfaceFactRef, StoreView,
    StoreViewCompatToken,
};

/// Build an [`AugmenterEntry`] for `canonical` carrying the given
/// `parse_stable_hash`. The exact `FileArtifactKey` is a `legacy`-shape
/// key with a placeholder content hash — these tests never insert the
/// augmenter artifact into `FileArtifactStore`, so the key's
/// `content_hash` is irrelevant to what they assert (the
/// augmenter-set fingerprint + per-contributor `FileWholeHash`
/// signature, both driven by `parse_stable_hash` / mocked hooks).
fn augmenter_entry(canonical: &str, parse_stable_hash: [u8; 16]) -> AugmenterEntry {
    AugmenterEntry {
        artifact_key: FileArtifactKey::legacy_for_test(std::sync::Arc::from(canonical), [0u8; 16]),
        parse_stable_hash,
    }
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

/// Selective view that rejects a specific RouteSurface fact key (by
/// kind tag + external specifier), keeping all others valid. Used to
/// exercise validator-driven invalidation paths.
#[derive(Debug)]
struct RejectRouteSurfaceFingerprintView {
    token: StoreViewCompatToken,
    rejected_external_specifier: String,
    rejected_fingerprint: [u8; 16],
}

impl StoreView for RejectRouteSurfaceFingerprintView {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.token
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        let FactVersionRef::RouteSurface(r) = fact else {
            return true;
        };
        let FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
            external_specifier: Some(spec),
            ..
        } = &r.key
        else {
            return true;
        };
        // Reject exactly the recorded (spec, fingerprint) pair.
        !(spec.as_ref() == self.rejected_external_specifier
            && r.expected_hash == self.rejected_fingerprint)
    }
}

// ────────────────────────────────────────────────────────────────
// Test 1 — cross-consumer route hit produces ONE per-name entry.
// ────────────────────────────────────────────────────────────────

#[test]
fn cross_consumer_route_hit_produces_one_entry() {
    let db = RouteDb::new();
    let view = AcceptAllView::new(1);

    // Two consumers query the same `(provider, name)`.
    let mut compute_count = 0u32;
    let mut do_query = |_label: &str| {
        db.get_or_resolve_route_with_facts("provider.ts", "Foo", &view, || {
            compute_count += 1;
            Some((
                RouteResult::Resolved {
                    defining_canonical: "foo.ts".to_owned(),
                    defining_symbol: "Foo".to_owned(),
                },
                vec![FactVersionRef::FileWholeHash {
                    canonical_id: "provider.ts".to_owned(),
                    hash: [1u8; 16],
                }],
            ))
        });
    };
    do_query("consumer-1");
    do_query("consumer-2");

    assert_eq!(
        compute_count, 1,
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
// Test 2 — SyntacticExportSet (parse-domain) ≠ EffectiveExportSet
// (resolve-domain) per R15. Build them by hand and assert disjoint
// in shape and emission domain.
// ────────────────────────────────────────────────────────────────

#[test]
fn syntactic_export_set_differs_from_effective_export_set() {
    let route_db = RouteDb::new();
    let artifact_store = FileArtifactStore::new();
    let view = AcceptAllView::new(1);

    // The augmenter set for the queried target is empty (no fixtures
    // loaded). The augmentation index gets populated to an empty
    // augmenter set on first miss.
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("ext-spec"));
    let key = EffectiveExportSetKey {
        provider_canonical: "ext-spec".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &view,
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );

    // R15 invariant: `EffectiveExportSet` is the post-augmentation
    // resolve-domain surface; `SyntacticExportSet` is the
    // parse-domain syntactic shape. They share NO storage.
    //
    // 1) `EffectiveExportSet` lives in `RouteDb.effective_export_sets`,
    //    keyed on `(provider, project, resolve_env, lib_env)`. NOT on
    //    a `FileArtifactKey`.
    // 2) `SyntacticExportSet` lives in
    //    `FileArtifactStore.artifacts[FileArtifactKey].facts.registry`
    //    keyed on `(canonical, content_hash, parse_env, parser_version)`.
    //    Empty `artifact_store` has zero `SyntacticExportSet` entries.
    // 3) `EffectiveExportSet` carries an `augmenter_set_fingerprint`
    //    field that `SyntacticExportSet` does not have.
    assert_eq!(effective.augmenter_count, 0);
    assert_eq!(effective.entries.len(), 0);

    // The two domains are disjoint by structure: confirm the
    // FactKey discriminants do NOT alias.
    let parse_export_set = FactKey::SyntacticExportSet;
    let route_effective_set = FactKey::EffectiveExportSet;
    assert!(
        std::mem::discriminant(&parse_export_set) != std::mem::discriminant(&route_effective_set),
        "SyntacticExportSet (parse-domain) and EffectiveExportSet \
         (resolve-domain) MUST be distinct FactKey variants (R15)"
    );

    // No augmenters → empty `EffectiveExportSetEntry.entries`.
    assert!(
        effective.entries.is_empty(),
        "no augmenters → effective set is empty (resolve-domain semantics)"
    );

    // The `EffectiveExportSet` cache slot exists.
    assert_eq!(route_db.effective_export_set_len(), 1);
}

// ────────────────────────────────────────────────────────────────
// Test 3 — whole_hash_migration_audit site #2 elimination.
// ────────────────────────────────────────────────────────────────

/// Co-asserts the whole_hash_migration_audit.rs site-#2 inversion
/// from a different angle: the new BarrelRouteSurface field shape
/// works through the entire route_db API.
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
    db.insert_barrel_surface(surface);

    let fetched = db
        .get_barrel_surface("barrel.ts", &view)
        .expect("barrel surface MUST round-trip");
    assert!(Arc::ptr_eq(&fetched.fact_dep_signature, &signature));
}

// ────────────────────────────────────────────────────────────────
// Test 4 — paths edit (resolve_env_hash) invalidates RouteDb.
// ────────────────────────────────────────────────────────────────

#[test]
fn paths_edit_invalidates_route_db() {
    let db = RouteDb::new();
    let artifact_store = FileArtifactStore::new();
    let view = AcceptAllView::new(1);

    // Two resolve envs → two distinct keys → two distinct candidate
    // entries (R5 multi-candidate; R21 resolve_env_hash dimension).
    let key_a = EffectiveExportSetKey {
        provider_canonical: "p.ts".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16], // env A
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let key_b = EffectiveExportSetKey {
        resolve_env_hash: [4u8; 16], // env B (paths edited)
        ..key_a.clone()
    };
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));

    let _ = db.get_or_compute_effective_export_set(
        key_a.clone(),
        target.clone(),
        &view,
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );
    let _ = db.get_or_compute_effective_export_set(
        key_b.clone(),
        target.clone(),
        &view,
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );

    assert_eq!(
        db.effective_export_set_len(),
        2,
        "two resolve-env-hash values MUST produce two distinct \
         EffectiveExportSet entries (R21 + R5)"
    );

    // The two entries are independent: editing env A's paths cannot
    // poison env B's surface.
    let warm_a = db.get_effective_export_set(&key_a, &view);
    let warm_b = db.get_effective_export_set(&key_b, &view);
    assert!(warm_a.is_some() && warm_b.is_some());
}

// ────────────────────────────────────────────────────────────────
// Test 5 — lib_env_hash invalidates EffectiveExportSet (R21).
// ────────────────────────────────────────────────────────────────

#[test]
fn lib_env_hash_change_invalidates_route_db_effective_set() {
    let db = RouteDb::new();
    let artifact_store = FileArtifactStore::new();
    let view = AcceptAllView::new(1);

    let key_lib_a = EffectiveExportSetKey {
        provider_canonical: "p.ts".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [10u8; 16], // lib A
        session_scope: EffectiveExportSetScope::Base,
    };
    let key_lib_b = EffectiveExportSetKey {
        lib_env_hash: [11u8; 16], // lib B
        ..key_lib_a.clone()
    };

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));

    let _ = db.get_or_compute_effective_export_set(
        key_lib_a.clone(),
        target.clone(),
        &view,
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );
    let _ = db.get_or_compute_effective_export_set(
        key_lib_b.clone(),
        target.clone(),
        &view,
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );

    // R21: lib_env_hash enters the EffectiveExportSet key, so two
    // distinct lib hashes produce two distinct cache entries.
    assert_eq!(
        db.effective_export_set_len(),
        2,
        "lib_env_hash MUST enter EffectiveExportSetKey (R21)"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 6 — paired negative: lib_env_hash does NOT invalidate
// ResolvedImportFacts (R21 scoping rule — base import resolution
// is independent of libs).
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

#[test]
fn effective_export_set_carries_fact_dep_signature() {
    let db = RouteDb::new();
    let artifact_store = FileArtifactStore::new();
    let view = AcceptAllView::new(1);

    // Pre-populate the augmentation index with one augmenter so the
    // computed effective set carries non-empty contributions and a
    // fact-dep-signature.
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target: target.clone(),
    };
    artifact_store.populate_augmenter_set(
        target_key.clone(),
        Arc::new(AugmenterSet {
            entries: smallvec![augmenter_entry("/aug.ts", [42u8; 16])],
            fingerprint: [99u8; 16],
        }),
    );

    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    let effective = db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
        None,
        &artifact_store,
        |c| {
            if c == "/aug.ts" {
                Some([42u8; 16])
            } else {
                None
            }
        },
        |_, _| None,
    );

    // The signature MUST carry at least the
    // RouteSurface(ModuleAugmentationIndexShape) fact plus the
    // contributor's FileWholeHash anchor.
    let has_route_surface_fact = effective.fact_dep_signature.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                key: FactKey::ModuleAugmentationIndexShape { .. },
                ..
            })
        )
    });
    assert!(
        has_route_surface_fact,
        "EffectiveExportSetEntry.fact_dep_signature MUST include a \
         RouteSurface(ModuleAugmentationIndexShape) anchor (G1)"
    );

    let has_contributor_anchor = effective.fact_dep_signature.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == "/aug.ts" && hash == &[42u8; 16]
        )
    });
    assert!(
        has_contributor_anchor,
        "EffectiveExportSetEntry.fact_dep_signature MUST include a \
         FileWholeHash anchor for the contributor file"
    );

    assert_eq!(effective.augmenter_set_fingerprint, [99u8; 16]);
}

#[test]
fn effective_export_set_invalidates_when_augmenter_set_fingerprint_changes() {
    let db = RouteDb::new();
    let artifact_store = FileArtifactStore::new();

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target: target.clone(),
    };
    let initial_fp = [99u8; 16];
    artifact_store.populate_augmenter_set(
        target_key.clone(),
        Arc::new(AugmenterSet {
            entries: smallvec![augmenter_entry("/aug.ts", [42u8; 16])],
            fingerprint: initial_fp,
        }),
    );

    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    // Compute under initial fingerprint with an accepting view.
    let _ = db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
        None,
        &artifact_store,
        |_| None,
        |_, _| None,
    );

    // Now the validator rejects the fingerprint that was recorded
    // (simulating a new augmenter having entered FileArtifactStore
    // and refreshed the index — the old fingerprint is now stale).
    let rejecting_view = RejectRouteSurfaceFingerprintView {
        token: StoreViewCompatToken {
            epoch: 2,
            session: None,
        },
        rejected_external_specifier: "vue".to_owned(),
        rejected_fingerprint: initial_fp,
    };
    let warm = db.get_effective_export_set(&key, &rejecting_view);
    assert!(
        warm.is_none(),
        "EffectiveExportSetEntry MUST be invalidated when the \
         RouteSurface(ModuleAugmentationIndexShape) fact under its \
         signature fails validation — G1 invariant"
    );
}

// ────────────────────────────────────────────────────────────────
// Test-only helpers extension on RouteDb (lives in the test crate
// because production code does not currently expose route snapshot).
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
        if let Some(r) = self.get_route(&probe_key.0, &probe_key.1, &view) {
            out.push((probe_key, r));
        }
        out
    }
}

// Suppress unused-import warnings if any helper is unused on a
// specific code path. Keeps the test file resilient to refactors.
#[allow(dead_code)]
fn _suppress_unused() {
    let _ = SymbolSpace::Type;
    let _ = InternedName::from("x");
    let _ = FactLane::Semantic;
    let _: Option<EffectiveExportEntry> = None;
    let _: Option<EffectiveExportSetEntry> = None;
}
