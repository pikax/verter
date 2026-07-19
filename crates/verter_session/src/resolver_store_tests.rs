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

// ── `hash_route_surface` — purity pin + per-state memoization ──

mod route_surface_hash {
    use super::FxHashMap;
    use crate::resolver_core::shallow_file_state::{
        ExportTarget, ImportTarget, ShallowFileState, WildcardReexport,
    };
    use rustc_hash::FxHashSet;
    use std::sync::Arc;
    use verter_semantic::analysis::Hash16;

    /// A routing surface exercising every dimension `hash_route_surface`
    /// digests: local exports, a named reexport (baked canonical +
    /// type-only flag), a wildcard edge, and an import target.
    pub(super) fn routed_state(whole_hash: Hash16) -> ShallowFileState {
        let exports = FxHashMap::from_iter([
            (
                "Local".to_string(),
                ExportTarget::Local {
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol_name: "Local".to_string(),
                },
            ),
            (
                "Renamed".to_string(),
                ExportTarget::Reexport {
                    source_specifier: "./dep".to_string(),
                    original_name: "Orig".to_string(),
                    canonical_id: "/src/dep.ts".to_string(),
                    is_type: true,
                },
            ),
        ]);
        let wildcard_reexports = vec![WildcardReexport {
            source_specifier: "./barrel".to_string(),
            canonical_id: "/src/barrel.ts".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        }];
        let import_locals = FxHashSet::from_iter(["Dep".to_string()]);
        let import_targets = FxHashMap::from_iter([(
            "Dep".to_string(),
            ImportTarget {
                source_specifier: "./dep".to_string(),
                imported_name: "Dep".to_string(),
                is_namespace: false,
                canonical_id: "/src/dep.ts".to_string(),
            },
        )]);
        ShallowFileState::routing_tables_only_for_test(
            whole_hash,
            exports,
            wildcard_reexports,
            import_locals,
            import_targets,
            Arc::new(
                verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory::default(),
            ),
        )
    }

    /// The hash is a pure function of the routing surface: an
    /// INDEPENDENTLY CONSTRUCTED identical state hashes identically, and
    /// any surface move (reexport retarget, content-hash move) moves it.
    #[test]
    fn matches_independent_recomputation_and_discriminates_surface_moves() {
        let state = routed_state([7u8; 16]);
        let independent = routed_state([7u8; 16]);
        assert_eq!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&independent),
            "identical routing surfaces must hash identically",
        );

        // Negative: a reexport RETARGET moves the hash — the surface is
        // digested WITH routing targets, never just export names. The
        // mutation happens strictly BEFORE the state's first hash (the
        // construction-time mutation window).
        let mut retargeted = routed_state([7u8; 16]);
        retargeted.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./dep".to_string(),
                original_name: "Orig".to_string(),
                canonical_id: "/src/dep.d.ts".to_string(),
                is_type: true,
            },
        );
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&retargeted),
            "a dependency retarget must move the route-surface hash",
        );

        // Negative: the owner content hash participates.
        let content_moved = routed_state([8u8; 16]);
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&content_moved),
            "a whole-hash move must move the route-surface hash",
        );
    }
}

// The memo test lives in the same module as the purity pin so both read
// the same `routed_state` fixture.
mod route_surface_hash_memo {
    use super::route_surface_hash::routed_state;
    use crate::resolver_core::shallow_file_state::ExportTarget;

    /// The memo populates on the state's FIRST hash and every later call
    /// returns the identical value; a CLONE starts with an EMPTY memo
    /// (fresh `OnceLock`) so its independent recomputation agrees on an
    /// unmutated clone and re-digests the clone's OWN surface after a
    /// clone-side mutation (never the donor's stale digest).
    #[test]
    fn memoizes_per_state_and_resets_on_clone() {
        let state = routed_state([9u8; 16]);
        assert!(
            state.route_surface_hash_memo().get().is_none(),
            "memo must start unpopulated",
        );
        let first = crate::resolver_store::hash_route_surface(&state);
        assert_eq!(
            state.route_surface_hash_memo().get(),
            Some(first),
            "first hash must populate the memo with the computed digest",
        );
        assert_eq!(
            first,
            crate::resolver_store::hash_route_surface(&state),
            "the memoized (second) call must return the identical value",
        );

        // Init path: a clone resets the memo and recomputes fresh — the
        // recomputation must agree with the donor's digest.
        let cloned = state.clone();
        assert!(
            cloned.route_surface_hash_memo().get().is_none(),
            "a clone must start with an EMPTY memo, not the donor's cached digest",
        );
        assert_eq!(
            first,
            crate::resolver_store::hash_route_surface(&cloned),
            "an unmutated clone's fresh computation must equal the donor's digest",
        );

        // Clone-then-mutate: the reset means a mutated clone hashes its
        // OWN surface. Carrying the donor's populated memo across the
        // clone would serve a stale digest here.
        let mut mutated = state.clone();
        mutated.exports.insert(
            "Extra".to_string(),
            ExportTarget::Local {
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol_name: "Extra".to_string(),
            },
        );
        assert_ne!(
            first,
            crate::resolver_store::hash_route_surface(&mutated),
            "a mutated clone must hash its own surface, not the donor's cached digest",
        );
    }
}

// ---------------------------------------------------------------------------
// Import-route target digest formula pin
// ---------------------------------------------------------------------------

/// Pins the exact `hash_import_route_targets` digest formula: per sorted
/// specifier, a `0u8` tag, the specifier, then the effective resolution as
/// an `Option<str>`-shaped hash (`Some(resolved | best-candidate)` /
/// known-miss `None`), split-hashed into the 16-byte pair. Guards the
/// allocation-free digest path against any formula drift — a digest change
/// here silently invalidates every recorded `ImportRoute` fact.
#[test]
fn import_route_target_digest_formula_is_pinned() {
    use std::hash::{Hash, Hasher};

    let mut resolutions: FxHashMap<String, crate::types::DependencyResolution> =
        FxHashMap::default();
    resolutions.insert(
        "./exact".to_string(),
        crate::types::DependencyResolution {
            specifier: "./exact".to_string(),
            resolved_canonical_id: Some("/w/exact.ts".to_string()),
            possible_canonical_ids: vec![],
        },
    );
    resolutions.insert(
        "./candidates".to_string(),
        crate::types::DependencyResolution {
            specifier: "./candidates".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec!["/w/c.js".to_string(), "/w/c.d.ts".to_string()],
        },
    );
    resolutions.insert(
        "./known-miss".to_string(),
        crate::types::DependencyResolution {
            specifier: "./known-miss".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![],
        },
    );

    // Reference digest: the historical owned-`Option<String>` formula,
    // written out longhand. `Option<String>` and `Option<&str>` hash
    // byte-identically, so the production digest must match EXACTLY.
    let mut entries: Vec<_> = resolutions.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let feed = |hasher: &mut rustc_hash::FxHasher| {
        for (specifier, resolution) in &entries {
            0u8.hash(hasher);
            specifier.hash(hasher);
            resolution
                .resolved_canonical_id
                .clone()
                .or_else(|| resolution.effective_target().map(str::to_string))
                .hash(hasher);
        }
    };
    let mut left = rustc_hash::FxHasher::default();
    0u8.hash(&mut left);
    feed(&mut left);
    let mut right = rustc_hash::FxHasher::default();
    1u8.hash(&mut right);
    feed(&mut right);
    let mut expected = [0u8; 16];
    expected[..8].copy_from_slice(&left.finish().to_le_bytes());
    expected[8..].copy_from_slice(&right.finish().to_le_bytes());

    assert_eq!(
        crate::resolver_store::hash_import_route_targets(&resolutions),
        expected,
        "route-target digest formula drifted — every ImportRoute fact would misvalidate"
    );

    // The digest DISCRIMINATES: dropping the known-miss entry changes it.
    resolutions.remove("./known-miss");
    assert_ne!(
        crate::resolver_store::hash_import_route_targets(&resolutions),
        expected
    );
}
