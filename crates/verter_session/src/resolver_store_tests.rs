//! Tests for [`crate::resolver_store::HostStoreView`] — the
//! session-overlay-aware fact validator: `validates` arms,
//! untracked-file optimistic accept, and the `hash_import_route_targets`
//! lazy-promotion invariant.

use crate::resolver_store::{hash_import_route_targets, HostStoreView};
use crate::types::DependencyResolution;
use rustc_hash::FxHashMap;

use crate::resolver_core::StoreView;

/// Files loaded as dependencies DURING resolution (after the store view
/// snapshot was taken) are not tracked in `whole_hashes`. The validated
/// cache must accept facts for these untracked files — otherwise every
/// access to a dependency falls through to the expensive permissive path.
#[test]
fn validates_accepts_untracked_file_whole_hash() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::from_iter([(
        "/src/Accordion.vue".to_string(),
        [1u8; 16],
    )]));

    // Tracked file with matching hash — should validate.
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/Accordion.vue".to_string(),
            hash: [1u8; 16],
        })
    );

    // Tracked file with mismatching hash — should reject.
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/Accordion.vue".to_string(),
            hash: [2u8; 16],
        })
    );

    // Untracked dependency file — should accept (loaded after view snapshot).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
            hash: [42u8; 16],
        }),
        "untracked dependency files should be accepted by the store view"
    );
}

/// DerivedFactHash::DirectSource for untracked files should be accepted
/// (same as FileWholeHash — it's a content-hash alias). Non-DirectSource
/// derived facts for untracked files should NOT be accepted — they are
/// invalidation signals (import routes, etc.) that must be explicitly
/// tracked to participate in validation.
#[test]
fn validates_derived_fact_hash_semantics() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::default());

    // DirectSource for untracked file — should accept (content-hash alias).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::DirectSource,
            hash: [99u8; 16],
        }),
        "DirectSource for untracked file should be accepted"
    );

    // Route for untracked file — should NOT accept (invalidation signal).
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::Route,
            hash: [99u8; 16],
        }),
        "Route derived fact for untracked file should NOT be accepted"
    );
}

/// Concurrent generations of the same key are distinguished by
/// per-candidate fact validation against the candidate's own
/// `fact_dep_signature` (see
/// `crates/verter_session/src/resolver_core/mod.rs`
/// `ValidatedFactCache` substrate). For untracked files, the
/// primary `validates` path accepts the cached hash because the
/// candidate was admitted from current workspace content.
#[test]
fn primary_validates_accepts_untracked_file_whole_hash() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::from_iter([(
        "/src/tracked.ts".to_string(),
        [1u8; 16],
    )]));

    // Tracked file — matches.
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/tracked.ts".to_string(),
            hash: [1u8; 16],
        })
    );

    // Tracked file — mismatched hash rejected.
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/tracked.ts".to_string(),
            hash: [2u8; 16],
        })
    );

    // Untracked file — accepted (multi-candidate
    // substrate relies on the candidate's own `fact_dep_signature`
    // to discriminate concurrent generations).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
            hash: [42u8; 16],
        }),
        "untracked files are accepted by primary validation in the multi-candidate substrate"
    );
}

#[test]
fn import_route_hash_ignores_lazy_promotion_to_same_effective_target() {
    let lazy = FxHashMap::from_iter([(
        "./types".to_string(),
        DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/types.d.ts".to_string(),
                "/src/types.ts".to_string(),
            ],
        },
    )]);
    let promoted = FxHashMap::from_iter([(
        "./types".to_string(),
        DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.d.ts".to_string()),
            possible_canonical_ids: vec![
                "/src/types.d.ts".to_string(),
                "/src/types.ts".to_string(),
            ],
        },
    )]);

    assert_eq!(
        hash_import_route_targets(&lazy),
        hash_import_route_targets(&promoted),
        "lazy promotion to the same effective canonical target should not invalidate ImportRoute facts",
    );
}

#[test]
fn import_route_hash_changes_when_effective_target_changes() {
    let before = FxHashMap::from_iter([(
        "./types".to_string(),
        DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/types.d.ts".to_string(),
                "/src/types.ts".to_string(),
            ],
        },
    )]);
    let after = FxHashMap::from_iter([(
        "./types".to_string(),
        DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec![
                "/src/types.d.ts".to_string(),
                "/src/types.ts".to_string(),
            ],
        },
    )]);

    assert_ne!(
        hash_import_route_targets(&before),
        hash_import_route_targets(&after),
        "changing the effective canonical target must still invalidate ImportRoute facts",
    );
}
