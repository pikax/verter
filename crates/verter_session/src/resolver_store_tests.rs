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

// ── FileSourceEnv strict source-env validation ──

mod file_source_env_validation {
    use super::*;
    use crate::file_artifact_store::FileArtifactKey;
    use crate::locator_identity::ParseEnvHash;
    use crate::resolver_core::FactVersionRef;
    use crate::resolver_store::SourceEnvIdentity;

    const CONTRIB: &str = "/contrib.d.ts";
    const CONTRIB_HASH: [u8; 16] = [5u8; 16];

    fn live_identity() -> SourceEnvIdentity {
        SourceEnvIdentity {
            parse_env_hash: ParseEnvHash::from_env_hash([3u8; 16]),
            parser_version: 2,
            file_language_id: FileArtifactKey::derived_file_language_id(CONTRIB),
        }
    }

    fn recorded_fact(
        canonical: &str,
        env_byte: u8,
        parser_version: u32,
        language_of: &str,
    ) -> FactVersionRef {
        FactVersionRef::FileSourceEnv {
            canonical_id: canonical.to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash([env_byte; 16]),
            parser_version,
            file_language_id: FileArtifactKey::derived_file_language_id(language_of),
        }
    }

    /// The recorded fact matching the [`live_identity`] plant.
    fn matching_fact() -> FactVersionRef {
        recorded_fact(CONTRIB, 3, 2, CONTRIB)
    }

    fn whole_hash_fact() -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: CONTRIB.to_string(),
            hash: CONTRIB_HASH,
        }
    }

    /// A view tracking the contributor's whole hash AND its live
    /// source-env identity.
    fn planted_view() -> HostStoreView {
        HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::from_iter([(CONTRIB.to_string(), live_identity())]),
            std::collections::HashSet::new(),
        )
    }

    #[test]
    fn file_source_env_validates_matching_live_identity() {
        let view = planted_view();
        assert!(
            view.validates(&matching_fact()),
            "a recorded source-env identity equal to the view-current identity must validate"
        );
    }

    #[test]
    fn file_source_env_rejects_canonical_id_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact("/other.d.ts", 3, 2, CONTRIB)),
            "a contributor canonical the view has no source-env identity for must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_parse_env_hash_mismatch_with_valid_whole_hash() {
        let view = planted_view();
        let stale = recorded_fact(CONTRIB, 4, 2, CONTRIB);
        // Isolation: the sibling whole-hash fact still validates —
        // rejection comes from the source-env branch alone.
        assert!(
            view.validates(&whole_hash_fact()),
            "sanity: the contributor FileWholeHash must still validate under this view"
        );
        assert!(
            !view.validates(&stale),
            "a recorded parse_env_hash differing from the view-current identity must reject"
        );
        assert!(
            !view.validates_fact_signature(&[whole_hash_fact(), stale]),
            "the full signature must reject on the source-env mismatch even though the \
             whole-hash fact still matches"
        );
    }

    #[test]
    fn file_source_env_rejects_parser_version_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact(CONTRIB, 3, 7, CONTRIB)),
            "a recorded parser_version differing from the view-current identity must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_file_language_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact(CONTRIB, 3, 2, "/contrib.vue")),
            "a recorded file_language_id differing from the view-current identity must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_missing_identity_even_when_whole_hash_matches() {
        // Whole hash tracked and matching, but NO source-env identity
        // for the contributor: the strict branch must reject (never
        // the optimistic untracked-accept the whole-hash arm applies).
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::default(),
            std::collections::HashSet::new(),
        );
        assert!(
            view.validates(&whole_hash_fact()),
            "sanity: the contributor FileWholeHash must validate under this view"
        );
        assert!(
            !view.validates(&matching_fact()),
            "a missing view-current source-env identity must reject strictly"
        );
    }

    #[test]
    fn file_source_env_rejects_tombstoned_canonical() {
        // Tombstoned wins even over a matching planted identity.
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::from_iter([(CONTRIB.to_string(), live_identity())]),
            std::collections::HashSet::from_iter([CONTRIB.to_string()]),
        );
        assert!(
            !view.validates(&matching_fact()),
            "a tombstoned contributor canonical must reject its source-env fact"
        );
    }

    #[test]
    fn file_source_env_rejects_untracked_canonical() {
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::default(),
            FxHashMap::default(),
            std::collections::HashSet::new(),
        );
        assert!(
            !view.validates(&matching_fact()),
            "an untracked contributor canonical must reject strictly, never optimistically accept"
        );
    }
}
