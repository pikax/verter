//! Module-augmentation stitching discrimination tests.
//!
//! Each test discriminates the stitching behaviour: a tree without
//! augmentation-index population, without augmenter-set fingerprint
//! observation in `EffectiveExportSet.fact_dep_signature`, and without
//! the `ModuleAugmentationStitched` audit event would FAIL; the
//! correct tree PASSES.
//!
//! Verify bullets covered here:
//!
//! - Module augmentation by target kind: each R29 archetype routes
//!   through the correct `AugmentationTargetKind` and stitches.
//! - Project isolation: augmenters in one project do NOT
//!   poison another project under the same syntactic specifier.
//! - Augmenter-set invalidation (G1): adding/removing an augmenter
//!   changes `ModuleAugmentationIndexShape.fingerprint`; downstream
//!   consumer's `EffectiveExportSet` cache entry invalidates.
//! - Editing an augmenting file invalidates the consumer.
//! - Editing an unrelated file does not.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::facts::registry::{InternedGlobPattern, InternedSpecifier};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, FileArtifactStore,
    FileArtifacts, ProjectIdentity, LEGACY_PARSER_VERSION,
};
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;
use verter_session::resolver_core::{
    EffectiveExportSetKey, EffectiveExportSetScope, FactVersionRef, RouteDb, StoreView,
    StoreViewCompatToken,
};

// ────────────────────────────────────────────────────────────────
// Test helpers
// ────────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture(name: &str) -> String {
    let path = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("tests")
        .join("fixtures")
        .join("path_precise")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
    Arc::new(verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default())
}

fn build_indexed_with_source(raw: &str, whole_hash: [u8; 16]) -> Arc<IndexedReady> {
    // Build the shallow inventory through the REAL binder so the typed
    // augmentation inventory (the single source of truth for augmentation
    // facts) is populated, exactly as production does.
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(raw);
    let shallow = ShallowFileState::from_analysis(whole_hash, empty_external(), Some(&env));
    Arc::new(IndexedReady {
        whole_hash,
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        edge_generation: 0,
        raw_source: Arc::from(raw),
        eval_source: Arc::from(""),
        cached_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

/// Insert a file artifact into `store` with parse-domain facts +
/// augmentations emitted from the fixture source. Returns the
/// `FileArtifactKey` so the caller can refer back to it.
fn insert_artifact_from_fixture(
    store: &FileArtifactStore,
    canonical: &str,
    fixture_name: &str,
    content_hash: [u8; 16],
) -> FileArtifactKey {
    let raw = fixture(fixture_name);
    let indexed = build_indexed_with_source(&raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let artifacts = Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    });
    let key = FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: [0u8; 16],
        parser_version: LEGACY_PARSER_VERSION,
    };
    store.insert_artifacts(key.clone(), artifacts);
    key
}

/// Insert a file with custom raw source (for the "edit unrelated
/// file" test where the source has NO augmentations).
fn insert_artifact_with_raw_source(
    store: &FileArtifactStore,
    canonical: &str,
    raw: &str,
    content_hash: [u8; 16],
) -> FileArtifactKey {
    let indexed = build_indexed_with_source(raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let artifacts = Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    });
    let key = FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: [0u8; 16],
        parser_version: LEGACY_PARSER_VERSION,
    };
    store.insert_artifacts(key.clone(), artifacts);
    key
}

/// Build an `Arc<FileArtifacts>` from raw source WITHOUT inserting it.
/// Used to pre-build a reusable payload for cheap re-entrant writes in
/// the resolver-off-guard test.
fn build_filler_artifacts(raw: &str, content_hash: [u8; 16]) -> Arc<FileArtifacts> {
    let indexed = build_indexed_with_source(raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    })
}

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

/// View that simulates a stale `ModuleAugmentationIndexShape`
/// observation — rejects any RouteSurface fact carrying the recorded
/// fingerprint for the external specifier `target_spec`.
#[derive(Debug)]
struct RejectStaleAugmenterFingerprint {
    token: StoreViewCompatToken,
    target_spec: String,
    stale_fingerprint: [u8; 16],
}

impl StoreView for RejectStaleAugmenterFingerprint {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.token
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        use verter_semantic::facts::FactKey;
        let FactVersionRef::RouteSurface(r) = fact else {
            return true;
        };
        let FactKey::ModuleAugmentationIndexShape {
            external_specifier: Some(s),
            ..
        } = &r.key
        else {
            return true;
        };
        !(s.as_ref() == self.target_spec && r.expected_hash == self.stale_fingerprint)
    }
}

// ────────────────────────────────────────────────────────────────
// Test 7 — external-specifier archetype stitches.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmentation_external_specifier_stitches() {
    let store = FileArtifactStore::new();
    let _key = insert_artifact_from_fixture(
        &store,
        "/aug-external.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );
    let route_db = RouteDb::new();
    let view = AcceptAllView::new(1);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
        None,
        &store,
        |_| Some([11u8; 16]),
        |_, _| None,
    );

    // Augmenter is `/aug-external.ts`; it contributes `foo` to
    // `ComponentOptions` per the fixture's `declare module "vue" {}`.
    assert_eq!(
        effective.augmenter_count, 1,
        "external-specifier archetype MUST produce exactly one augmenter contribution"
    );
    let contributor = &effective.entries[0].contributor_canonical;
    assert_eq!(contributor.as_ref(), "/aug-external.ts");
}

// ────────────────────────────────────────────────────────────────
// Test 8 — resolved-relative-canonical archetype stitches.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmentation_resolved_relative_canonical_stitches() {
    let store = FileArtifactStore::new();
    let augmenter_canonical = "/dir/aug-relative.ts";
    let _key = insert_artifact_from_fixture(
        &store,
        augmenter_canonical,
        "module_augmentation_relative.ts",
        [12u8; 16],
    );
    let route_db = RouteDb::new();
    let view = AcceptAllView::new(1);

    // The augmenter's specifier `./local` should resolve to
    // `/dir/local.ts` against the augmenter's directory `/dir`.
    let resolved_target = Arc::<str>::from("/dir/local.ts");
    let target = AugmentationTargetKind::ResolvedRelativeCanonical(Arc::clone(&resolved_target));
    let key = EffectiveExportSetKey {
        provider_canonical: "/dir/local.ts".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
        None,
        &store,
        |_| Some([12u8; 16]),
        |augmenter, specifier| {
            // Simple lexical join: assume augmenter's directory is
            // `/dir/`, augmenter's specifier is `./local`. Build
            // `/dir/local.ts`.
            if augmenter == augmenter_canonical && specifier == "./local" {
                Some(Arc::<str>::from("/dir/local.ts"))
            } else {
                None
            }
        },
    );

    assert_eq!(
        effective.augmenter_count, 1,
        "resolved-relative-canonical archetype MUST produce exactly one augmenter contribution"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 9 — wildcard-ambient archetype stitches.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmentation_wildcard_ambient_stitches() {
    let store = FileArtifactStore::new();
    let _key = insert_artifact_from_fixture(
        &store,
        "/aug-wild.ts",
        "module_augmentation_wildcard.ts",
        [13u8; 16],
    );
    let route_db = RouteDb::new();
    let view = AcceptAllView::new(1);

    let target = AugmentationTargetKind::WildcardAmbient(InternedGlobPattern::from("*.css"));
    let key = EffectiveExportSetKey {
        provider_canonical: "*.css".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
        None,
        &store,
        |_| Some([13u8; 16]),
        |_, _| None,
    );

    assert_eq!(
        effective.augmenter_count, 1,
        "wildcard-ambient archetype MUST produce exactly one augmenter contribution"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 10 — global augmentation archetype stitches.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmentation_global_stitches() {
    let store = FileArtifactStore::new();
    let _key = insert_artifact_from_fixture(
        &store,
        "/aug-global.ts",
        "module_augmentation_global.ts",
        [14u8; 16],
    );
    let route_db = RouteDb::new();
    let view = AcceptAllView::new(1);

    let target = AugmentationTargetKind::GlobalAugmentation;
    let key = EffectiveExportSetKey {
        provider_canonical: "$global".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
        None,
        &store,
        |_| Some([14u8; 16]),
        |_, _| None,
    );

    assert_eq!(
        effective.augmenter_count, 1,
        "global-augmentation archetype MUST produce exactly one augmenter contribution"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 11 — cross-project augmenter isolation.
// ────────────────────────────────────────────────────────────────

#[test]
fn cross_project_augmenter_isolation() {
    let store = FileArtifactStore::new();

    // Augmenter A loaded into project 1's resolve_env.
    let _key_a = insert_artifact_from_fixture(
        &store,
        "/proj1/aug.ts",
        "module_augmentation_external.ts",
        [21u8; 16],
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let project_identity_1 = ProjectIdentity([1u8; 16]);
    let project_identity_2 = ProjectIdentity([2u8; 16]);
    let resolve_env_1 = [3u8; 16];
    let resolve_env_2 = [4u8; 16];

    let target_key_p1 = AugmentationTargetKey {
        project_identity: project_identity_1,
        resolve_env_hash: resolve_env_1,
        lib_env_hash: [0u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: target.clone(),
    };
    let target_key_p2 = AugmentationTargetKey {
        project_identity: project_identity_2,
        resolve_env_hash: resolve_env_2,
        lib_env_hash: [0u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: target.clone(),
    };

    // Populate project 1's index with augmenter A.
    let set_p1 = store.ensure_augmentation_index_populated(&target_key_p1, |_, _| None, None);
    // Populate project 2's index — augmenter A is the same file in
    // FileArtifactStore, but the project-isolation contract says: the
    // index entry is keyed by `AugmentationTargetKey { project,
    // resolve_env, lib_env, target }`, so two distinct project keys
    // produce TWO distinct entries.
    let set_p2 = store.ensure_augmentation_index_populated(&target_key_p2, |_, _| None, None);

    // Distinct index entries.
    assert_eq!(
        store.augmentation_index_len(),
        2,
        "two distinct AugmentationTargetKey entries MUST coexist \
         (project_identity + resolve_env_hash isolation)"
    );

    // Both projects see augmenter A's contribution because A's
    // ModuleAugmentationFact matches the target specifier — the
    // production resolve-time check is the only thing that would
    // partition. The KEY property: the two index entries are
    // SEPARATE — augmenters present in project A's set do NOT
    // automatically appear in project B's set, because each set
    // was scanned + populated under its own key.
    assert!(
        !Arc::ptr_eq(&set_p1, &set_p2),
        "project 1 and project 2 augmenter sets MUST be distinct \
         allocations (project isolation prevents Arc identity overlap)"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 12 — publishing a new augmenter through the ORDINARY artifact
// API invalidates the augmentation index, so the downstream
// `EffectiveExportSet` fingerprint transitions and a stale-fingerprint
// view refuses the cached entry. Drives the REAL publish path
// (`FileArtifactStore::insert_artifacts`) — NO direct index-refresh
// call — so it characterizes production lifecycle behavior.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmenter_publish_invalidates_downstream_via_artifact_api() {
    let store = FileArtifactStore::new();

    // Step 1 — workspace starts with only the primary augmenter.
    let _primary_key = insert_artifact_from_fixture(
        &store,
        "/primary-aug.ts",
        "module_augmentation_added_augmenter.ts",
        [31u8; 16],
    );

    let route_db = RouteDb::new();
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    // Cold compute under view A; captures the initial fingerprint.
    let effective_initial = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
        None,
        &store,
        |c| {
            if c == "/primary-aug.ts" {
                Some([31u8; 16])
            } else {
                None
            }
        },
        |_, _| None,
    );
    let initial_fingerprint = effective_initial.augmenter_set_fingerprint;
    assert_eq!(effective_initial.augmenter_count, 1);

    // Step 2 — publish the secondary augmenter through the ORDINARY
    // artifact API. The new artifact's augmentations target the same
    // `"vue"` specifier, so the publish invalidates the augmentation
    // index entry for that target (lifecycle coherence rail) — there is
    // NO direct index-refresh call. The next `ensure` cold-rebuilds and
    // the fingerprint transitions.
    let _secondary_key = insert_artifact_from_fixture(
        &store,
        "/secondary-aug.ts",
        "module_augmentation_added_augmenter_secondary.ts",
        [32u8; 16],
    );

    // The previously-recorded fingerprint is now stale: a view that
    // refuses it should fail to validate the cached entry.
    let stale_view = RejectStaleAugmenterFingerprint {
        token: StoreViewCompatToken {
            epoch: 2,
            session: None,
            validity_fingerprint: 0,
        },
        target_spec: "vue".to_owned(),
        stale_fingerprint: initial_fingerprint,
    };
    let warm = route_db.get_effective_export_set(&key, &stale_view);
    assert!(
        warm.is_none(),
        "an augmenter-set change MUST invalidate the downstream `EffectiveExportSet` \
         consumer (G1) — the cached entry's `RouteSurface(ModuleAugmentationIndexShape)` \
         fact MUST fail validation under the stale fingerprint"
    );

    // Step 3 — recompute under a view that knows the post-publish
    // fingerprint (`new_fingerprint`) but rejects the stale one. The
    // new effective set MUST include BOTH augmenters.
    //
    // The publish in Step 2 INVALIDATED (dropped) the index entry, so the
    // next `ensure` cold-rebuilds it from the now-current artifact corpus
    // (base ∪ {secondary}) — this is exactly what the next production query
    // does, and what a fresh view would then snapshot. A pre-fix tree (where
    // `insert_artifacts` did NOT invalidate the index) warm-returns the stale
    // 1-augmenter set here, so `new_fingerprint == initial_fingerprint` and
    // the `assert_ne!` below FAILS.
    let new_target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: target.clone(),
    };
    let new_set = store.ensure_augmentation_index_populated(&new_target_key, |_, _| None, None);
    assert_eq!(
        new_set.entries.len(),
        2,
        "publish MUST invalidate the index so the cold-rebuild includes BOTH \
         augmenters; a stale warm entry was returned"
    );
    let new_fingerprint = new_set.fingerprint;
    assert_ne!(
        new_fingerprint, initial_fingerprint,
        "augmenter-set fingerprint MUST transition after the new augmenter is \
         published through the ordinary artifact API (G1)"
    );

    #[derive(Debug)]
    struct OnlyNewFingerprintView {
        token: StoreViewCompatToken,
        target_spec: String,
        new_fingerprint: [u8; 16],
    }
    impl StoreView for OnlyNewFingerprintView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }
        fn validates(&self, fact: &FactVersionRef) -> bool {
            use verter_semantic::facts::FactKey;
            let FactVersionRef::RouteSurface(r) = fact else {
                return true;
            };
            let FactKey::ModuleAugmentationIndexShape {
                external_specifier: Some(s),
                ..
            } = &r.key
            else {
                return true;
            };
            if s.as_ref() != self.target_spec {
                return true;
            }
            r.expected_hash == self.new_fingerprint
        }
    }

    let post_refresh_view = OnlyNewFingerprintView {
        token: StoreViewCompatToken {
            epoch: 3,
            session: None,
            validity_fingerprint: 0,
        },
        target_spec: "vue".to_owned(),
        new_fingerprint,
    };

    let effective_refreshed = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &post_refresh_view,
        None,
        &store,
        |c| match c {
            "/primary-aug.ts" => Some([31u8; 16]),
            "/secondary-aug.ts" => Some([32u8; 16]),
            _ => None,
        },
        |_, _| None,
    );
    assert_eq!(
        effective_refreshed.augmenter_count, 2,
        "recomputed effective set MUST include BOTH augmenters"
    );
    assert_eq!(
        effective_refreshed.augmenter_set_fingerprint, new_fingerprint,
        "recomputed entry MUST carry the post-refresh fingerprint"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 13 — editing an augmenting file invalidates the consumer.
// ────────────────────────────────────────────────────────────────

#[test]
fn edit_augmenting_file_invalidates_consumer() {
    let store = FileArtifactStore::new();

    // Load augmenter at content_hash A.
    let original_hash = [41u8; 16];
    let _key_a = insert_artifact_from_fixture(
        &store,
        "/aug.ts",
        "module_augmentation_external.ts",
        original_hash,
    );

    let route_db = RouteDb::new();
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    // Cold compute records contributor_whole_hash = original_hash.
    let _ = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
        None,
        &store,
        |c| {
            if c == "/aug.ts" {
                Some(original_hash)
            } else {
                None
            }
        },
        |_, _| None,
    );

    // Now simulate an edit: the augmenter's whole_hash changes.
    // A view that snapshotted the old hash will refuse validation
    // because the `FileWholeHash` anchor for the augmenter under
    // the consumer's signature points at the old hash but the view
    // tracks the new hash.
    #[derive(Debug)]
    struct EditedAugmenterView {
        token: StoreViewCompatToken,
        edited_canonical: String,
        new_hash: [u8; 16],
    }
    impl StoreView for EditedAugmenterView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }
        fn validates(&self, fact: &FactVersionRef) -> bool {
            match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash } => {
                    if canonical_id == &self.edited_canonical {
                        hash == &self.new_hash
                    } else {
                        true
                    }
                }
                // RouteSurface facts validate trivially in this test.
                _ => true,
            }
        }
    }

    let edited_view = EditedAugmenterView {
        token: StoreViewCompatToken {
            epoch: 2,
            session: None,
            validity_fingerprint: 0,
        },
        edited_canonical: "/aug.ts".to_owned(),
        new_hash: [99u8; 16], // post-edit hash
    };
    let warm = route_db.get_effective_export_set(&key, &edited_view);
    assert!(
        warm.is_none(),
        "editing an augmenting file MUST invalidate downstream \
         `EffectiveExportSet` consumer — the per-contributor \
         FileWholeHash anchor under the consumer's signature MUST \
         fail validation"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 14 — control: editing an unrelated file does NOT invalidate.
// ────────────────────────────────────────────────────────────────

#[test]
fn edit_unrelated_file_does_not_invalidate_consumer() {
    let store = FileArtifactStore::new();

    // Augmenter at /aug.ts.
    let aug_hash = [51u8; 16];
    let _aug_key = insert_artifact_from_fixture(
        &store,
        "/aug.ts",
        "module_augmentation_external.ts",
        aug_hash,
    );

    // Unrelated file at /unrelated.ts with no augmentations.
    let _unrelated_key = insert_artifact_with_raw_source(
        &store,
        "/unrelated.ts",
        "export const greeting = 'hi';\nexport {};\n",
        [52u8; 16],
    );

    let route_db = RouteDb::new();
    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    let _ = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
        None,
        &store,
        |c| {
            if c == "/aug.ts" {
                Some(aug_hash)
            } else if c == "/unrelated.ts" {
                Some([52u8; 16])
            } else {
                None
            }
        },
        |_, _| None,
    );

    // Now an edit to /unrelated.ts. The augmentation index does not
    // refresh (the unrelated file has no augmentations). The
    // consumer's signature did NOT include a FileWholeHash anchor
    // for /unrelated.ts. A view that reports the unrelated file's
    // new hash MUST keep the consumer's entry valid.
    #[derive(Debug)]
    struct UnrelatedEditedView {
        token: StoreViewCompatToken,
        aug_canonical: String,
        aug_hash: [u8; 16],
    }
    impl StoreView for UnrelatedEditedView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }
        fn validates(&self, fact: &FactVersionRef) -> bool {
            match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash } => {
                    // The augmenter's hash MUST still match (no edit to it).
                    if canonical_id == &self.aug_canonical {
                        hash == &self.aug_hash
                    } else {
                        // Unrelated files default-accept.
                        true
                    }
                }
                _ => true,
            }
        }
    }

    let view = UnrelatedEditedView {
        token: StoreViewCompatToken {
            epoch: 2,
            session: None,
            validity_fingerprint: 0,
        },
        aug_canonical: "/aug.ts".to_owned(),
        aug_hash,
    };
    let warm = route_db.get_effective_export_set(&key, &view);
    assert!(
        warm.is_some(),
        "editing an unrelated file MUST NOT invalidate consumer — \
         control assertion that our signature is narrow per R14/R28"
    );
}

// ────────────────────────────────────────────────────────────────
// Test 15 — re-keyed augmenter with an unchanged decl skeleton is
// NOT silently dropped from the stitched surface.
//
// The augmentation index captures each augmenter's EXACT
// `FileArtifactKey` (content-addressed) at index-population time. The
// augmenter-set fingerprint folds over `parse_stable_hash` (the decl
// skeleton), NOT `content_hash`. So a member-body edit to an augmenter
// — content hash changes, decl skeleton (hence `parse_stable_hash`,
// hence the fingerprint) unchanged — does NOT invalidate the cached
// `AugmenterSet`: its `AugmenterEntry.artifact_key` keeps pointing at
// the PRE-edit content hash. A same-canonical edit drains that
// pre-edit `FileArtifactKey`, so the stitch's exact-key
// `get_artifacts` lookup misses.
//
// - **Against `d7b3ddd0f`**: the stitch sees the exact-key miss and
//   `continue`s — the augmenter's declarations are silently dropped,
//   so the recomputed effective set has an EMPTY `entries` list while
//   `augmenter_count` still reports 1. This test FAILS (the
//   `!entries.is_empty()` assertion trips).
// - **Post-fix**: the stitch self-heals on the miss — it re-derives
//   the augmenter's CURRENT exact key from the scheduler-authoritative
//   `contributor_whole_hash` oracle and reads pinned to it, so the
//   augmenter's `foo` contribution is still stitched. This test
//   PASSES.
//
// The contract: a stale captured key must NOT silently drop an
// augmenter whose content version is still materialised.
// ────────────────────────────────────────────────────────────────

#[test]
fn rekeyed_augmenter_with_unchanged_skeleton_is_not_dropped() {
    let store = FileArtifactStore::new();
    let augmenter_canonical = "/aug-rekey.ts";

    // Step 1 — augmenter present at the PRE-edit content hash.
    let pre_edit_hash = [61u8; 16];
    let pre_edit_key = insert_artifact_from_fixture(
        &store,
        augmenter_canonical,
        "module_augmentation_external.ts",
        pre_edit_hash,
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    // Cold compute on the first RouteDb — this populates the
    // augmentation index on `store`, capturing the augmenter's EXACT
    // pre-edit `FileArtifactKey`.
    let route_db_initial = RouteDb::new();
    let effective_initial = route_db_initial.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
        None,
        &store,
        |c| {
            if c == augmenter_canonical {
                Some(pre_edit_hash)
            } else {
                None
            }
        },
        |_, _| None,
    );
    assert_eq!(
        effective_initial.augmenter_count, 1,
        "pre-edit cold compute MUST see exactly one augmenter"
    );
    assert!(
        !effective_initial.entries.is_empty(),
        "pre-edit cold compute MUST stitch the augmenter's `foo` contribution"
    );
    let pre_edit_fingerprint = effective_initial.augmenter_set_fingerprint;

    // Step 2 — a same-canonical member-body edit. The fixture source
    // is byte-identical, so the rebuilt `IndexedReady` has the SAME
    // decl skeleton and therefore the SAME `parse_stable_hash` — only
    // the content hash advances. `remove` drains every prior
    // `FileArtifactKey` for the canonical (the same drain a
    // same-canonical `FileArtifactStore::insert` performs), then the
    // augmenter is reparsed under the NEW content-hash key.
    let post_edit_hash = [62u8; 16];
    store.remove(augmenter_canonical);
    let post_edit_key = insert_artifact_from_fixture(
        &store,
        augmenter_canonical,
        "module_augmentation_external.ts",
        post_edit_hash,
    );

    // The pre-edit key is genuinely drained; the post-edit key is the
    // only live artifact. (Confirms the test reproduces the exact
    // failure mode rather than a coexisting-versions one.)
    assert!(
        store.get_artifacts(&pre_edit_key).is_none(),
        "pre-edit FileArtifactKey MUST be drained after the re-key"
    );
    assert!(
        store.get_artifacts(&post_edit_key).is_some(),
        "post-edit FileArtifactKey MUST be the live artifact"
    );
    // The decl skeleton is unchanged, so `parse_stable_hash` — and
    // hence the augmenter-set fingerprint — does NOT move. The cached
    // `AugmenterSet` is therefore NOT invalidated and still carries
    // the stale pre-edit `artifact_key`.
    assert_eq!(
        pre_edit_key.content_hash, pre_edit_hash,
        "pre-edit key carries the pre-edit content hash"
    );
    assert_ne!(
        post_edit_key.content_hash, pre_edit_key.content_hash,
        "the re-key MUST advance the content-hash dimension"
    );

    // Step 3 — recompute the effective export set on a FRESH RouteDb
    // so the `effective_export_sets` cache is empty and the cold
    // stitch runs. The augmentation index lives on the shared `store`
    // and warm-hits, handing the stitch the cached `AugmenterSet`
    // whose `AugmenterEntry.artifact_key` is the now-stale pre-edit
    // key. `contributor_whole_hash` reports the augmenter's CURRENT
    // (post-edit) content hash — the scheduler authority.
    let route_db_recompute = RouteDb::new();
    let effective_recomputed = route_db_recompute.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(2),
        None,
        &store,
        |c| {
            if c == augmenter_canonical {
                Some(post_edit_hash)
            } else {
                None
            }
        },
        |_, _| None,
    );

    // The discriminating assertion. Pre-fix the stale exact-key miss
    // is a silent `continue`, so `entries` is EMPTY; post-fix the
    // stitch self-heals via the scheduler-authoritative current key,
    // so the augmenter's `foo` contribution is still present.
    assert!(
        !effective_recomputed.entries.is_empty(),
        "a re-keyed augmenter whose decl skeleton is unchanged MUST NOT \
         be silently dropped from the stitched surface — the stitch \
         MUST self-heal the stale captured key via the \
         scheduler-authoritative current content hash"
    );
    assert!(
        effective_recomputed
            .entries
            .iter()
            .any(|e| e.contributor_canonical.as_ref() == augmenter_canonical),
        "the recomputed effective set MUST attribute the stitched \
         contribution to the re-keyed augmenter"
    );
    assert_eq!(
        effective_recomputed.augmenter_count, 1,
        "augmenter count is unchanged across the re-key (one augmenter)"
    );
    // The fingerprint is invariant under the decl-skeleton-preserving
    // edit — the self-heal advances only the `artifact_key`.
    assert_eq!(
        effective_recomputed.augmenter_set_fingerprint, pre_edit_fingerprint,
        "the augmenter-set fingerprint MUST be invariant under a \
         decl-skeleton-preserving re-key"
    );

    // The self-heal also writes the refreshed exact key back into the
    // cached `AugmenterSet`, so a subsequent stitch hits the fast
    // exact-key path with no further refresh.
    let target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: target.clone(),
    };
    let healed_set = store
        .get_augmenter_set(&target_key)
        .expect("augmentation index MUST still hold an entry");
    assert_eq!(
        healed_set.entries.len(),
        1,
        "the healed augmenter set MUST still hold exactly one entry"
    );
    assert_eq!(
        healed_set.entries[0].artifact_key.content_hash, post_edit_hash,
        "the self-heal MUST write the refreshed (post-edit) exact key \
         back into the cached AugmenterSet"
    );
    assert_eq!(
        healed_set.fingerprint, pre_edit_fingerprint,
        "writing back the refreshed key MUST preserve the augmenter-set \
         fingerprint"
    );
}

// ────────────────────────────────────────────────────────────────
// Overlay-correct session contract — a session view stitches its OWN
// overlay augmenters.
//
// `RouteDb::get_or_compute_effective_export_set` derives its
// augmentation-index population identity + overlay discriminator from the
// active `SessionView` through the shared
// `session_view::augmentation_population_for_view` — the SAME derivation the
// body stitch uses. The base-only `session.is_none()` assert is RETIRED, and
// the session branch no longer returns a base-only set under a session key:
// a session view keys its augmenter set under `Session(overlay-set
// fingerprint)` and the cold scan unions the session's own overlay augmenters
// (matched by the overlay discriminator) with base, while a base view stays
// base-only.
//
// The guard `no_effective_export_set_base_only_session_assert`
// (`g_misc0/critical_rules_have_guards.rs`) statically pins the assert deletion
// so it cannot be re-introduced.
// ────────────────────────────────────────────────────────────────

/// A `StoreView` carrying a session identity — `compat_token().session`
/// is `Some`. Accepts every fact (so the cold compute is reached and warm
/// validation never spuriously misses in this test).
#[derive(Debug)]
struct SessionScopedView {
    token: StoreViewCompatToken,
}

impl SessionScopedView {
    fn new(epoch: u64, session: u64) -> Self {
        Self {
            token: StoreViewCompatToken {
                epoch,
                session: Some(session),
                validity_fingerprint: 0,
            },
        }
    }
}

impl StoreView for SessionScopedView {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.token
    }
    fn validates(&self, _fact: &FactVersionRef) -> bool {
        true
    }
}

/// A minimal overlay-bearing `SessionView` whose `fingerprint()` derives the
/// overlay artifact discriminator that the session's overlay augmenter was
/// inserted under. Only `fingerprint()` participates in
/// `augmentation_population_for_view`, so the other accessors return the
/// base-only defaults.
#[derive(Debug)]
struct OverlayFingerprintView {
    fingerprint: u64,
    project_identity: ProjectIdentity,
    env_hashes: verter_session::session_view::EnvHashes,
}

impl verter_session::session_view::SessionView for OverlayFingerprintView {
    fn source(&self, _canonical: &str) -> Option<Arc<str>> {
        None
    }
    fn content_hash_for(&self, _canonical: &str) -> Option<verter_session::Hash16> {
        None
    }
    fn project_identity(&self) -> ProjectIdentity {
        self.project_identity
    }
    fn env_hashes(&self) -> &verter_session::session_view::EnvHashes {
        &self.env_hashes
    }
    fn resolved_import_facts(
        &self,
        _canonical: &str,
    ) -> Option<Arc<verter_session::resolved_import_facts::ResolvedImportFacts>> {
        None
    }
    fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// Discriminator (overlay-aware augmentation index, names stitch): a session
/// view passed to `get_or_compute_effective_export_set` stitches its OWN overlay
/// augmenter (matched by the overlay discriminator) UNIONED with base, while a
/// base view sees ONLY the base augmenter.
///
/// - **Against the pre-fix tree** (`overlay_discriminator: None`,
///   `Session(session_id)`): the session call scans base artifacts only, so the
///   overlay augmenter `/aug-overlay.ts` NEVER contributes — this test FAILS.
/// - **Post-fix tree**: the session call threads the real overlay discriminator
///   (derived from `fingerprint()` via `augmentation_population_for_view`), so
///   the overlay augmenter contributes to the session surface and is absent from
///   the base surface — PASSES.
#[test]
fn effective_export_set_session_view_stitches_overlay_augmenter() {
    let store = FileArtifactStore::new();

    // Base augmenter (legacy key) that `declare module "vue" {}` augments.
    let _base_key = insert_artifact_from_fixture(
        &store,
        "/aug-base.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );

    // Session-overlay augmenter: a DIFFERENT file, keyed under the non-legacy
    // overlay discriminator derived from fingerprint 7. It augments the same
    // `"vue"` target but exists ONLY in this session's overlay.
    let fingerprint: u64 = 7;
    let overlay_discriminator =
        verter_session::session_view::overlay_artifact_discriminator_for_fingerprint(fingerprint);
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source(&raw, [99u8; 16]);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let overlay_key = FileArtifactKey {
        canonical: Arc::from("/aug-overlay.ts"),
        content_hash: [99u8; 16],
        parse_env_hash: overlay_discriminator,
        parser_version: LEGACY_PARSER_VERSION,
    };
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts {
            indexed,
            facts: Arc::new(emission.facts),
            parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }),
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };
    let whole_hash = |c: &str| match c {
        "/aug-base.ts" => Some([11u8; 16]),
        "/aug-overlay.ts" => Some([99u8; 16]),
        _ => None,
    };

    // SESSION read: derives the overlay discriminator from the session view's
    // fingerprint and unions the overlay augmenter with base. Separate RouteDb
    // instances keep the base and session result caches from cross-pollinating
    // here; the shared-`RouteDb` warm-slot separation (the content-free
    // `EffectiveExportSetKey.session_scope`, R6) is exercised by the
    // session-scope-keyed warm-cache tests below.
    let session_store_view = SessionScopedView::new(1, 42);
    let overlay_session = OverlayFingerprintView {
        fingerprint,
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let session_db = RouteDb::new();
    let session_effective = session_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &session_store_view,
        Some(&overlay_session),
        &store,
        whole_hash,
        |_, _| None,
    );

    // BASE read: no session view → base-only augmenter set.
    let base_store_view = AcceptAllView::new(1);
    let base_db = RouteDb::new();
    let base_effective = base_db.get_or_compute_effective_export_set(
        key,
        target,
        &base_store_view,
        None,
        &store,
        whole_hash,
        |_, _| None,
    );

    let session_contributors: Vec<&str> = session_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();
    let base_contributors: Vec<&str> = base_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();

    // The overlay augmenter contributes to the SESSION surface…
    assert!(
        session_contributors.contains(&"/aug-overlay.ts"),
        "session view MUST stitch its overlay augmenter; got {session_contributors:?}"
    );
    assert!(
        session_contributors.contains(&"/aug-base.ts"),
        "session view must also include the base augmenter (union with base); got {session_contributors:?}"
    );
    // …and is ABSENT from the BASE surface.
    assert!(
        !base_contributors.contains(&"/aug-overlay.ts"),
        "base view MUST NOT see the session-overlay augmenter; got {base_contributors:?}"
    );
    assert!(
        base_contributors.contains(&"/aug-base.ts"),
        "base view must include the base augmenter; got {base_contributors:?}"
    );
    assert!(
        session_effective.augmenter_count > base_effective.augmenter_count,
        "session augmenter set (base ∪ overlay) must be larger than base-only"
    );
}

/// Discriminator (overlay-aware augmentation index): a
/// `Session(id)` population augmenter set UNIONS the session's own overlay
/// augmenters (matched by the overlay discriminator) with base, while the
/// `Base` population set sees ONLY base augmenters. A session overlay's
/// `declare module` augmenter NEVER appears in the base index.
///
/// - **Against the pre-deletion tree** (no `population` dimension, scan filters
///   `is_legacy()` only): the overlay (non-legacy) augmenter is invisible to
///   BOTH calls, so the session set never contains it — this test FAILS.
/// - **Post-change tree**: the session scan (`Some(discriminator)`) includes
///   the overlay augmenter; the base scan (`None`) excludes it — PASSES.
#[test]
fn session_overlay_augmenter_isolated_from_base_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();

    // Base augmenter (legacy key) that `declare module "vue" {}` augments.
    let _base_key = insert_artifact_from_fixture(
        &store,
        "/aug-base.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );

    // Session-overlay augmenter: a DIFFERENT file, keyed under a non-legacy
    // overlay discriminator `D` (the `parse_env_hash` dimension) derived from
    // the overlay-set fingerprint. It augments the same `"vue"` target but
    // exists ONLY in this session's overlay.
    //
    // Two distinct things both derive from the overlay-set fingerprint and must
    // not be conflated: (a) the CONTENT-ADDRESSED augmentation-index population
    // KEY `AugmentationPopulation::Session(fingerprint)`, and (b) the scan
    // DISCRIMINATOR (the `parse_env_hash` byte tag) that matches overlay
    // artifacts. Both use the SAME fingerprint here so the index slot and the
    // matched artifacts agree — this is NOT a raw session id.
    let overlay_fingerprint: u64 = 7;
    let overlay_discriminator =
        verter_session::session_view::overlay_artifact_discriminator_for_fingerprint(
            overlay_fingerprint,
        );
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source(&raw, [99u8; 16]);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let overlay_key = FileArtifactKey {
        canonical: Arc::from("/aug-overlay.ts"),
        content_hash: [99u8; 16],
        parse_env_hash: overlay_discriminator,
        parser_version: LEGACY_PARSER_VERSION,
    };
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts {
            indexed,
            facts: Arc::new(emission.facts),
            parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }),
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let base_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target: target.clone(),
    };
    let session_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        // CONTENT-ADDRESSED index population: keyed by the overlay-set
        // fingerprint (the SAME value the scan discriminator above derives
        // from), NOT a raw session id.
        population: AugmentationPopulation::Session(overlay_fingerprint),
        target,
    };

    let base_set = store.ensure_augmentation_index_populated(&base_key, |_, _| None, None);
    let session_set = store.ensure_augmentation_index_populated(
        &session_key,
        |_, _| None,
        Some(overlay_discriminator),
    );

    let base_canonicals: Vec<&str> = base_set
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    let session_canonicals: Vec<&str> = session_set
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();

    // BASE sees only the base augmenter — the overlay augmenter is invisible.
    assert_eq!(base_canonicals, vec!["/aug-base.ts"]);
    assert!(
        !base_canonicals.contains(&"/aug-overlay.ts"),
        "base index MUST NOT see the session-overlay augmenter"
    );

    // SESSION sees base UNIONED with the overlay augmenter.
    assert!(
        session_canonicals.contains(&"/aug-base.ts"),
        "session index must include the base augmenter (union with base)"
    );
    assert!(
        session_canonicals.contains(&"/aug-overlay.ts"),
        "session index MUST include the session-overlay augmenter"
    );
    // The two populations are distinct entries (fingerprints differ).
    assert_ne!(
        base_set.fingerprint, session_set.fingerprint,
        "base and session augmenter sets must be distinct (overlay isolation)"
    );
}

/// Publishing a BASE augmenter through the ORDINARY artifact API MUST
/// invalidate the `Session` augmentation-index entries that include that base
/// augmenter — driven by the lifecycle coherence rail, NOT a direct
/// index-refresh call.
///
/// A `Session` augmenter set is base ∪ overlay, and
/// [`FileArtifactStore::ensure_augmentation_index_populated`] warm-returns
/// an existing set before rescanning. The `Session`-population key carries the
/// overlay-set fingerprint, which a BASE membership change does NOT move — so
/// without lifecycle invalidation the stale `Session` set lingers (its
/// `ModuleAugmentationIndexShape` fingerprint never moves) and a later session
/// `ensure` / `EffectiveExportSet` warm-hits WITHOUT the new base contributor.
///
/// - **Pre-fix tree** (`insert_artifacts` does NOT invalidate the index): the
///   re-`ensure` warm-hits the stale base ∪ overlay set, so the newly
///   published base augmenter `/aug-base-2.ts` is ABSENT — this test FAILS.
/// - **Post-fix tree** (publish invalidates the matching `Base` AND `Session`
///   entries population-agnostically): the re-`ensure` cold-rescans base ∪
///   overlay, so the new base augmenter is PRESENT and overlay isolation is
///   preserved — PASSES.
#[test]
fn base_augmenter_publish_invalidates_session_augmentation_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();
    let overlay_fingerprint: u64 = 7;
    let overlay_discriminator = seed_base_and_overlay_augmenters(&store, overlay_fingerprint);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let session_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Session(overlay_fingerprint),
        target: target.clone(),
    };
    let base_key = AugmentationTargetKey {
        population: AugmentationPopulation::Base,
        ..session_key.clone()
    };

    // Populate BOTH the base and session entries. Session = base ∪ overlay.
    let _base_set = store.ensure_augmentation_index_populated(&base_key, |_, _| None, None);
    let session_before = store.ensure_augmentation_index_populated(
        &session_key,
        |_, _| None,
        Some(overlay_discriminator),
    );
    let before: Vec<&str> = session_before
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        before.contains(&"/aug-base.ts") && before.contains(&"/aug-overlay.ts"),
        "session set must start as base ∪ overlay; got {before:?}"
    );
    assert!(
        !before.contains(&"/aug-base-2.ts"),
        "new base augmenter is not in the store yet; got {before:?}"
    );

    // Publish a NEW base augmenter for the same `"vue"` target through the
    // ORDINARY artifact API. The publish itself invalidates the matching
    // index entries (base AND session) — there is NO direct index-refresh
    // call.
    let _new_base_key = insert_artifact_from_fixture(
        &store,
        "/aug-base-2.ts",
        "module_augmentation_external.ts",
        [22u8; 16],
    );

    // Re-ensure the session entry. Post-fix the prior entry was
    // invalidated by the publish, so this cold-rescans base ∪ overlay and
    // picks up the new base augmenter. Pre-fix it warm-hits the stale set.
    let session_after = store.ensure_augmentation_index_populated(
        &session_key,
        |_, _| None,
        Some(overlay_discriminator),
    );
    let after: Vec<&str> = session_after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();

    assert!(
        after.contains(&"/aug-base-2.ts"),
        "session augmentation index MUST reflect the base augmenter published \
         through the ordinary artifact API; a stale session entry was \
         warm-returned. got {after:?}"
    );
    // Overlay isolation preserved: base ∪ overlay still both present.
    assert!(
        after.contains(&"/aug-overlay.ts") && after.contains(&"/aug-base.ts"),
        "session set must remain base ∪ overlay after the base change; got {after:?}"
    );
    // And the base index never absorbed the session-overlay augmenter.
    let base_after = store.ensure_augmentation_index_populated(&base_key, |_, _| None, None);
    let base_after_canonicals: Vec<&str> = base_after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        !base_after_canonicals.contains(&"/aug-overlay.ts"),
        "base index MUST NOT absorb the session-overlay augmenter; got {base_after_canonicals:?}"
    );
    assert!(
        base_after_canonicals.contains(&"/aug-base-2.ts"),
        "base index must fold in the added base augmenter; got {base_after_canonicals:?}"
    );
}

/// REMOVING a base augmenter through the ORDINARY evict API
/// (`FileArtifactStore::remove_canonical`) MUST invalidate the index
/// entry it contributed to, so the next `ensure` cold-rebuilds WITHOUT
/// the removed augmenter.
///
/// - **Pre-fix tree** (`remove_canonical` does NOT invalidate the index):
///   the re-`ensure` warm-hits the stale 2-augmenter set, so `/secondary.ts`
///   is still PRESENT after removal — this test FAILS.
/// - **Post-fix tree** (evict invalidates the index using the removed
///   augmenter's facts): the re-`ensure` cold-rescans the now-current corpus
///   and `/secondary.ts` is ABSENT — PASSES.
#[test]
fn base_augmenter_remove_invalidates_index_via_evict() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();
    let _primary = insert_artifact_from_fixture(
        &store,
        "/primary.ts",
        "module_augmentation_external.ts",
        [51u8; 16],
    );
    let _secondary = insert_artifact_from_fixture(
        &store,
        "/secondary.ts",
        "module_augmentation_external.ts",
        [52u8; 16],
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target,
    };

    // Warm the index entry: base ∪ {primary, secondary}.
    let before = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let before_canonicals: Vec<&str> = before
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        before_canonicals.contains(&"/primary.ts") && before_canonicals.contains(&"/secondary.ts"),
        "index must start with BOTH augmenters; got {before_canonicals:?}"
    );

    // Remove `/secondary.ts` through the ordinary evict API.
    let removed = store.remove_canonical("/secondary.ts");
    assert_eq!(removed, 1, "exactly one artifact key must be drained");

    // Re-ensure: post-fix the evict invalidated the index, so this
    // cold-rebuilds WITHOUT the removed augmenter.
    let after = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let after_canonicals: Vec<&str> = after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        after_canonicals.contains(&"/primary.ts"),
        "surviving augmenter must remain; got {after_canonicals:?}"
    );
    assert!(
        !after_canonicals.contains(&"/secondary.ts"),
        "removed augmenter MUST be dropped from the index after evict; a stale \
         warm entry was returned. got {after_canonicals:?}"
    );
    assert_ne!(
        before.fingerprint, after.fingerprint,
        "augmenter-set fingerprint MUST transition after the evict"
    );
}

/// Publishing a base augmenter through the PRODUCTION legacy publish
/// primitive (`FileArtifactStore::insert`, the path
/// `host_manage::prepared_decl` uses at materialisation time) MUST
/// invalidate the index entry it contributes to.
///
/// This pins the EXACT production base-publish primitive (not just the
/// content-addressed `insert_artifacts`): `insert` emits the augmentations
/// from the `IndexedReady` via `FileArtifacts::with_indexed` and folds them
/// through the same lifecycle coherence rail.
///
/// - **Pre-fix tree** (`insert` does NOT invalidate): the re-`ensure`
///   warm-hits the stale 1-augmenter set — FAILS.
/// - **Post-fix tree**: the publish invalidates, the re-`ensure`
///   cold-rebuilds with both augmenters — PASSES.
#[test]
fn base_augmenter_legacy_insert_invalidates_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();

    // Publish the first augmenter through the legacy `insert` primitive.
    let primary_raw = fixture("module_augmentation_external.ts");
    store.insert(
        Arc::from("/primary.ts"),
        build_indexed_with_source(&primary_raw, [61u8; 16]),
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target,
    };

    let before = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    assert_eq!(
        before.entries.len(),
        1,
        "index must start with exactly the first augmenter"
    );

    // Publish a SECOND augmenter through the same legacy primitive.
    let secondary_raw = fixture("module_augmentation_external.ts");
    store.insert(
        Arc::from("/secondary.ts"),
        build_indexed_with_source(&secondary_raw, [62u8; 16]),
    );

    let after = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let after_canonicals: Vec<&str> = after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        after_canonicals.contains(&"/primary.ts") && after_canonicals.contains(&"/secondary.ts"),
        "legacy `insert` publish MUST invalidate the index so the cold-rebuild \
         folds in the new augmenter; got {after_canonicals:?}"
    );
}

// ────────────────────────────────────────────────────────────────
// Artifact-only EVICTION paths (LRU floor + per-canonical retention)
// must invalidate the augmentation index too.
//
// `remove`/`remove_artifacts`/`remove_canonical` are the *public* drop
// surfaces — those are wired into the index-invalidation rail. But
// `FileArtifactStore::evict_lru_promoted` and
// `enforce_per_canonical_retention` drop entries from `self.artifacts`
// internally (driven by `evict_unreachable_artifacts_with_policy` on
// memory-pressure / long-session sweeps). If those internal drops bypass
// the rail, an evicted augmenter's `AugmenterSet` survives stale and the
// next `EffectiveExportSet` stitch republishes a stale fingerprint over
// an incomplete export surface.
//
// These tests drive the REAL eviction methods and assert the index
// transitions. They FAIL on the pre-fix tree (eviction bypassed the rail)
// and PASS once every artifact-removal path funnels through the single
// invalidating chokepoint.
// ────────────────────────────────────────────────────────────────

/// LRU floor eviction of an augmenter artifact MUST invalidate the
/// augmentation-index entry it contributed to.
///
/// - **Pre-fix tree** (`evict_lru_promoted` drops the entry without
///   invalidating): the re-`ensure` warm-hits the stale 2-augmenter set,
///   so `/secondary.ts` is still PRESENT — FAILS.
/// - **Post-fix tree** (LRU eviction funnels through the invalidating
///   chokepoint): the re-`ensure` cold-rebuilds WITHOUT the evicted
///   augmenter — PASSES.
#[test]
fn lru_eviction_of_augmenter_invalidates_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();
    // Insert `/secondary.ts` FIRST (older access tick) so it is the LRU
    // victim, then `/primary.ts` (newer tick) which must survive.
    let _secondary = insert_artifact_from_fixture(
        &store,
        "/secondary.ts",
        "module_augmentation_external.ts",
        [71u8; 16],
    );
    let _primary = insert_artifact_from_fixture(
        &store,
        "/primary.ts",
        "module_augmentation_external.ts",
        [72u8; 16],
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target,
    };

    let before = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let before_canonicals: Vec<&str> = before
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        before_canonicals.contains(&"/primary.ts") && before_canonicals.contains(&"/secondary.ts"),
        "index must start with BOTH augmenters; got {before_canonicals:?}"
    );

    // Memory-pressure LRU floor down to 1 entry: drops the oldest-tick
    // entry (`/secondary.ts`). promote_threshold = 0 → pure recency.
    store.evict_lru_promoted(1, 0);
    assert_eq!(
        store.len(),
        1,
        "LRU floor must have dropped exactly one artifact"
    );

    let after = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let after_canonicals: Vec<&str> = after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        after_canonicals.contains(&"/primary.ts"),
        "surviving augmenter must remain; got {after_canonicals:?}"
    );
    assert!(
        !after_canonicals.contains(&"/secondary.ts"),
        "LRU-evicted augmenter MUST be dropped from the index; a stale warm \
         entry was returned. got {after_canonicals:?}"
    );
    assert_ne!(
        before.fingerprint, after.fingerprint,
        "augmenter-set fingerprint MUST transition after the LRU eviction"
    );
}

/// Per-canonical retention eviction of an augmenter artifact MUST
/// invalidate the augmentation-index entry it contributed to.
///
/// `enforce_per_canonical_retention(0)` drops every variant of every
/// canonical — the most aggressive retention sweep. The store empties,
/// so the only correct post-eviction index is the empty set.
///
/// - **Pre-fix tree** (retention drops entries without invalidating): the
///   re-`ensure` warm-hits the stale non-empty set — FAILS.
/// - **Post-fix tree** (retention funnels through the invalidating
///   chokepoint): the re-`ensure` cold-rebuilds the empty set — PASSES.
#[test]
fn retention_eviction_of_augmenter_invalidates_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();
    let _primary = insert_artifact_from_fixture(
        &store,
        "/primary.ts",
        "module_augmentation_external.ts",
        [81u8; 16],
    );
    let _secondary = insert_artifact_from_fixture(
        &store,
        "/secondary.ts",
        "module_augmentation_external.ts",
        [82u8; 16],
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target,
    };

    let before = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    assert_eq!(
        before.entries.len(),
        2,
        "index must start with both augmenters"
    );

    // Retention 0 drops every variant of every canonical.
    store.enforce_per_canonical_retention(0);
    assert_eq!(
        store.len(),
        0,
        "retention(0) must drain the store; the test relies on this to \
         exercise the retention removal path"
    );

    let after = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    assert!(
        after.entries.is_empty(),
        "retention eviction emptied the store, so the index MUST cold-rebuild \
         to the empty set; a stale warm entry was returned. got {:?}",
        after
            .entries
            .iter()
            .map(|e| e.canonical().as_ref())
            .collect::<Vec<_>>()
    );
    assert_ne!(
        before.fingerprint, after.fingerprint,
        "augmenter-set fingerprint MUST transition from non-empty to empty"
    );
}

/// The SEQUENCE: an artifact-only eviction followed by a
/// later "normal" delete of the SAME canonical. After the eviction the
/// canonical has NO artifact left in the store, so the subsequent
/// `remove_canonical` finds nothing to collect and can invalidate
/// nothing. Therefore the eviction ITSELF must have invalidated the
/// index — there is no downstream delete that can heal it.
///
/// - **Pre-fix tree**: the LRU eviction bypassed the rail AND the later
///   delete has no facts to invalidate the stale `"vue"` entry, so the
///   re-`ensure` warm-hits a set that still lists the evicted augmenter
///   — FAILS.
/// - **Post-fix tree**: the LRU eviction invalidated at drop time, so the
///   index is already coherent and the no-op delete changes nothing —
///   PASSES.
#[test]
fn artifact_eviction_then_unrelated_delete_keeps_index_coherent() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();
    let _secondary = insert_artifact_from_fixture(
        &store,
        "/secondary.ts",
        "module_augmentation_external.ts",
        [91u8; 16],
    );
    let _primary = insert_artifact_from_fixture(
        &store,
        "/primary.ts",
        "module_augmentation_external.ts",
        [92u8; 16],
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: AugmentationPopulation::Base,
        target,
    };

    let before = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    assert_eq!(before.entries.len(), 2, "index starts with both augmenters");

    // Artifact-only eviction of `/secondary.ts` (oldest tick).
    store.evict_lru_promoted(1, 0);
    assert_eq!(store.len(), 1, "LRU dropped one artifact");

    // Later "normal" delete of the SAME canonical. It is already gone, so
    // this drains zero entries and has zero augmentation facts to feed the
    // invalidation rail — it cannot heal a stale index entry.
    let drained = store.remove_canonical("/secondary.ts");
    assert_eq!(
        drained, 0,
        "the canonical was already evicted, so the delete drains nothing"
    );

    let after = store.ensure_augmentation_index_populated(&key, |_, _| None, None);
    let after_canonicals: Vec<&str> = after
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();
    assert!(
        !after_canonicals.contains(&"/secondary.ts"),
        "the evicted augmenter MUST be absent: the eviction had to invalidate \
         the index itself, because the later delete has no facts left to do \
         it. got {after_canonicals:?}"
    );
    assert!(
        after_canonicals.contains(&"/primary.ts"),
        "the surviving augmenter must remain; got {after_canonicals:?}"
    );
    assert_ne!(
        before.fingerprint, after.fingerprint,
        "fingerprint MUST transition after the augmenter eviction"
    );
}

// ────────────────────────────────────────────────────────────────
// Session-scope-keyed warm cache — base/session entries occupy distinct
// `EffectiveExportSetKey` slots on the SAME `RouteDb`.
//
// `effective_export_set_session_view_stitches_overlay_augmenter` above
// proves the cold scan is overlay-correct, but it uses SEPARATE
// `RouteDb` instances for the base and session reads, so it can never
// exercise the warm-cache collision. The hazard the deleted base-only
// assert guarded is reintroduced at the cache layer if
// `EffectiveExportSetKey` is scope-blind: a base-populated warm entry
// then satisfies a session lookup (base-as-session) and vice versa,
// because both reads hash to the same slot.
//
// These two tests share ONE `RouteDb`. Both views' `validates()`
// accepts every fact, so warm validation never spuriously misses — the
// ONLY thing that can separate the base and session results is the
// cache key carrying the CONTENT-FREE `session_scope` dimension
// (`EffectiveExportSetScope`, derived from `compat_token().session`;
// the overlay-set content fingerprint never enters this query-identity
// key — R6).
//
// - **Pre-fix tree** (`EffectiveExportSetKey` has no session-
//   distinguishing field): the first read populates the shared slot, the
//   second read hashes to the SAME slot and warm-hits the wrong-scope
//   entry — so the session read sees base-only augmenters / the base read
//   sees the session overlay augmenter. Both asserts below FAIL.
// - **Post-fix tree**: base keys under `Base`, session keys under
//   `Session(scope_id)`; the second read misses the warm slot and
//   cold-computes its own scope. PASSES.
// ────────────────────────────────────────────────────────────────

/// Seed a shared store with a base augmenter (`/aug-base.ts`, legacy
/// key) and a session-overlay augmenter (`/aug-overlay.ts`, keyed under
/// the overlay discriminator for `fingerprint`). Both augment `"vue"`.
fn seed_base_and_overlay_augmenters(
    store: &FileArtifactStore,
    fingerprint: u64,
) -> verter_session::Hash16 {
    let _base_key = insert_artifact_from_fixture(
        store,
        "/aug-base.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );

    let overlay_discriminator =
        verter_session::session_view::overlay_artifact_discriminator_for_fingerprint(fingerprint);
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source(&raw, [99u8; 16]);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let overlay_key = FileArtifactKey {
        canonical: Arc::from("/aug-overlay.ts"),
        content_hash: [99u8; 16],
        parse_env_hash: overlay_discriminator,
        parser_version: LEGACY_PARSER_VERSION,
    };
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts {
            indexed,
            facts: Arc::new(emission.facts),
            parsed_edges: Arc::new(verter_session::file_artifact_store::ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }),
    );
    overlay_discriminator
}

fn aug_whole_hash(c: &str) -> Option<verter_session::Hash16> {
    match c {
        "/aug-base.ts" => Some([11u8; 16]),
        "/aug-overlay.ts" => Some([99u8; 16]),
        _ => None,
    }
}

/// Base-first → session-second on a SHARED `RouteDb`: the base read
/// populates the warm slot; the session read MUST still stitch its
/// overlay augmenter (not warm-hit the base-only entry).
#[test]
fn effective_export_set_warm_base_entry_does_not_satisfy_session_lookup() {
    let store = FileArtifactStore::new();
    let fingerprint: u64 = 7;
    let _overlay_discriminator = seed_base_and_overlay_augmenters(&store, fingerprint);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    let route_db = RouteDb::new();

    // BASE read first — populates the warm slot with the base-only set.
    let base_store_view = AcceptAllView::new(1);
    let base_effective = route_db.get_or_compute_effective_export_set(
        EffectiveExportSetKey {
            session_scope: EffectiveExportSetScope::Base,
            ..key.clone()
        },
        target.clone(),
        &base_store_view,
        None,
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let base_contributors: Vec<&str> = base_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();
    assert!(
        !base_contributors.contains(&"/aug-overlay.ts"),
        "base read must see base-only augmenters; got {base_contributors:?}"
    );

    // SESSION read second on the SAME db — must NOT warm-hit the
    // base-only entry; must stitch the overlay augmenter.
    let session_store_view = SessionScopedView::new(1, 42);
    let overlay_session = OverlayFingerprintView {
        fingerprint,
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let session_effective = route_db.get_or_compute_effective_export_set(
        EffectiveExportSetKey {
            // Content-free session scope (the producer overwrites this from
            // the view's `compat_token().session`); set for documentation.
            session_scope: EffectiveExportSetScope::Session(42),
            ..key
        },
        target,
        &session_store_view,
        Some(&overlay_session),
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let session_contributors: Vec<&str> = session_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();

    assert!(
        session_contributors.contains(&"/aug-overlay.ts"),
        "session read on a shared RouteDb must stitch its overlay augmenter, \
         NOT warm-hit the base-only entry; got {session_contributors:?}"
    );
    assert!(
        session_contributors.contains(&"/aug-base.ts"),
        "session read must still union the base augmenter; got {session_contributors:?}"
    );
}

/// Session-first → base-second on a SHARED `RouteDb`: the session read
/// populates the warm slot with the base ∪ overlay set; the base read
/// MUST still see the base-only surface (no overlay augmenter bleed).
#[test]
fn effective_export_set_warm_session_entry_does_not_satisfy_base_lookup() {
    let store = FileArtifactStore::new();
    let fingerprint: u64 = 7;
    let _overlay_discriminator = seed_base_and_overlay_augmenters(&store, fingerprint);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Base,
    };

    let route_db = RouteDb::new();

    // SESSION read first — populates the warm slot with base ∪ overlay.
    let session_store_view = SessionScopedView::new(1, 42);
    let overlay_session = OverlayFingerprintView {
        fingerprint,
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let session_effective = route_db.get_or_compute_effective_export_set(
        EffectiveExportSetKey {
            // Content-free session scope (overwritten by the producer from
            // the view's `compat_token().session`); set for documentation.
            session_scope: EffectiveExportSetScope::Session(42),
            ..key.clone()
        },
        target.clone(),
        &session_store_view,
        Some(&overlay_session),
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let session_contributors: Vec<&str> = session_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();
    assert!(
        session_contributors.contains(&"/aug-overlay.ts"),
        "session read must stitch its overlay augmenter; got {session_contributors:?}"
    );

    // BASE read second on the SAME db — must NOT warm-hit the session
    // entry; must see base-only.
    let base_store_view = AcceptAllView::new(1);
    let base_effective = route_db.get_or_compute_effective_export_set(
        EffectiveExportSetKey {
            session_scope: EffectiveExportSetScope::Base,
            ..key
        },
        target,
        &base_store_view,
        None,
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let base_contributors: Vec<&str> = base_effective
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();

    assert!(
        !base_contributors.contains(&"/aug-overlay.ts"),
        "base read on a shared RouteDb must NOT see the session overlay \
         augmenter (no base-as-session bleed); got {base_contributors:?}"
    );
    assert!(
        base_contributors.contains(&"/aug-base.ts"),
        "base read must include the base augmenter; got {base_contributors:?}"
    );
}

// ════════════════════════════════════════════════════════════════
// R6 — `EffectiveExportSetKey` is keyed by a CONTENT-FREE session
// scope, NOT the overlay-set content fingerprint.
//
// `EffectiveExportSetKey` is a QUERY-IDENTITY cache key, so R6 forbids
// any content/version-derived value in it. The session-scope dimension
// (`EffectiveExportSetScope { Base, Session(scope_id) }`) carries ONLY
// the orthogonal, content-free session identity
// (`StoreViewCompatToken::session`). Overlay CONTENT identity is rooted
// on the VALUE via the per-contributor `FileWholeHash` anchors + the
// `ModuleAugmentationIndexShape` augmenter-set fingerprint fact,
// revalidated against the live view on every warm hit.
//
// The two tests below pin the two halves of that contract:
//   1. A within-session overlay CONTENT edit invalidates the warm entry
//      THROUGH the value's facts (not through a different key).
//   2. An UNRELATED session change (the overlay-set fingerprint moves
//      but the augmenter content is unchanged) WARM-HITS the same slot
//      — proving the content fingerprint is NOT smuggled into the key.
// ════════════════════════════════════════════════════════════════

/// A session-scoped `StoreView` (`compat_token().session == Some(id)`)
/// that validates `FileWholeHash` facts against a caller-supplied
/// `(canonical → whole_hash)` map and accepts every other fact
/// (`RouteSurface` augmenter-set-shape facts validate trivially: the
/// augmenter SET membership is unchanged in these tests, only CONTENT
/// moves). A `FileWholeHash` for a canonical absent from the map is
/// refused, so every referenced contributor MUST appear in the map.
#[derive(Debug)]
struct SessionContentView {
    session: u64,
    whole_hashes: FxHashMap<String, [u8; 16]>,
}

impl StoreView for SessionContentView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 1,
            session: Some(self.session),
            validity_fingerprint: 0,
        }
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => {
                self.whole_hashes.get(canonical_id) == Some(hash)
            }
            // RouteSurface (`ModuleAugmentationIndexShape`) accepted: the
            // augmenter SET is unchanged here, so its fingerprint fact
            // would validate anyway — the discriminator is the
            // per-contributor `FileWholeHash` content rail.
            _ => true,
        }
    }
}

/// FIX A part 1 — within the SAME content-free session scope, editing
/// the overlay augmenter's CONTENT invalidates the warm
/// `EffectiveExportSet` through the value's `FileWholeHash` fact rail.
///
/// The cold read stores the entry under `Session(42)` and records a
/// `FileWholeHash(/aug-overlay.ts, [99;16])` anchor. A second view with
/// the SAME session scope (42) but a NEW overlay content hash for
/// `/aug-overlay.ts` warm-looks-up the SAME slot — and MUST miss,
/// because the recorded anchor no longer validates.
///
/// Discriminates: if the per-contributor `FileWholeHash` anchor were
/// dropped (or warm validation became accept-all), the stale warm entry
/// would be returned and this assert FAILS. The lookup hitting the same
/// slot under the SAME `Session(42)` key (despite the content change)
/// is exactly the content-free-key property — overlay CONTENT is rooted
/// on the value, not the key.
#[test]
fn effective_export_set_same_session_overlay_content_edit_invalidates_via_facts() {
    let store = FileArtifactStore::new();
    let fingerprint: u64 = 7;
    let _overlay_discriminator = seed_base_and_overlay_augmenters(&store, fingerprint);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        // Overwritten by the producer from the view's content-free
        // session scope; set here for documentation.
        session_scope: EffectiveExportSetScope::Session(42),
    };

    let route_db = RouteDb::new();

    // Cold read under session scope 42 (overlay fingerprint 7): stores
    // the base ∪ overlay surface under `Session(42)` with a
    // `FileWholeHash(/aug-overlay.ts, [99;16])` anchor.
    let session_store_view = SessionScopedView::new(1, 42);
    let overlay_session = OverlayFingerprintView {
        fingerprint,
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let cold = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &session_store_view,
        Some(&overlay_session),
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let cold_contributors: Vec<&str> = cold
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();
    assert!(
        cold_contributors.contains(&"/aug-overlay.ts"),
        "cold session read must stitch the overlay augmenter; got {cold_contributors:?}"
    );

    // Warm lookup with the SAME session scope (42) but the overlay
    // augmenter edited to a NEW content hash. Same `Session(42)` slot,
    // but the recorded `FileWholeHash(/aug-overlay.ts, [99;16])` anchor
    // no longer validates → the warm entry MUST be refused.
    let mut edited_hashes = FxHashMap::default();
    edited_hashes.insert("/aug-base.ts".to_owned(), [11u8; 16]);
    edited_hashes.insert("/aug-overlay.ts".to_owned(), [77u8; 16]); // edited
    let edited_view = SessionContentView {
        session: 42,
        whole_hashes: edited_hashes,
    };
    let warm = route_db.get_effective_export_set(&key, &edited_view);
    assert!(
        warm.is_none(),
        "a within-session overlay CONTENT edit MUST invalidate the warm \
         EffectiveExportSet through the per-contributor FileWholeHash fact \
         rail — a stale prior-overlay value must NOT be returned"
    );
}

/// FIX A part 2 — the key is content-free: an UNRELATED session change
/// (the overlay-set fingerprint moves, but the augmenter CONTENT is
/// unchanged) WARM-HITS the same `Session(scope)` slot instead of
/// cold-recomputing a different fingerprint slot.
///
/// Read 1 cold-computes under session scope 42 with overlay fingerprint
/// 7 (the overlay augmenter is seeded under fingerprint-7's
/// discriminator) and stores the base ∪ overlay surface under
/// `Session(42)`. Read 2 drives the producer again under the SAME
/// session scope 42 but a DIFFERENT overlay fingerprint (999 — no
/// overlay augmenter is seeded under its discriminator). The augmenter
/// CONTENT is unchanged, so read 2's view validates read 1's facts and
/// WARM-HITS — returning the base ∪ overlay surface.
///
/// Discriminates: were the overlay-set content fingerprint smuggled
/// into the key (the pre-fix R6 violation), read 2's distinct
/// fingerprint would key a DIFFERENT slot, miss the warm entry, and
/// cold-rescan under fingerprint-999's discriminator — which finds NO
/// overlay augmenter, so the result would NOT contain `/aug-overlay.ts`.
/// Asserting the overlay augmenter survives proves the content-free key.
#[test]
fn effective_export_set_content_free_key_warm_hits_across_unrelated_fingerprint_change() {
    let store = FileArtifactStore::new();
    let fingerprint_a: u64 = 7;
    let _overlay_discriminator = seed_base_and_overlay_augmenters(&store, fingerprint_a);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        session_scope: EffectiveExportSetScope::Session(42),
    };

    let route_db = RouteDb::new();

    // Read 1 — cold under session scope 42, overlay fingerprint 7.
    let session_store_view = SessionScopedView::new(1, 42);
    let overlay_a = OverlayFingerprintView {
        fingerprint: fingerprint_a,
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let _ = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &session_store_view,
        Some(&overlay_a),
        &store,
        aug_whole_hash,
        |_, _| None,
    );

    // Read 2 — SAME session scope 42 but a DIFFERENT overlay fingerprint
    // (999). Augmenter content unchanged, so the recorded facts still
    // validate. A `SessionContentView` (session 42) reports the
    // unchanged whole-hashes so the warm validation passes.
    let mut unchanged_hashes = FxHashMap::default();
    unchanged_hashes.insert("/aug-base.ts".to_owned(), [11u8; 16]);
    unchanged_hashes.insert("/aug-overlay.ts".to_owned(), [99u8; 16]);
    let read2_view = SessionContentView {
        session: 42,
        whole_hashes: unchanged_hashes,
    };
    let overlay_b = OverlayFingerprintView {
        fingerprint: 999, // unrelated overlay edit moved the fingerprint
        project_identity: ProjectIdentity([1u8; 16]),
        env_hashes: verter_session::session_view::EnvHashes::default(),
    };
    let read2 = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &read2_view,
        Some(&overlay_b),
        &store,
        aug_whole_hash,
        |_, _| None,
    );
    let read2_contributors: Vec<&str> = read2
        .entries
        .iter()
        .map(|e| e.contributor_canonical.as_ref())
        .collect();
    assert!(
        read2_contributors.contains(&"/aug-overlay.ts"),
        "a content-free session-scope key must WARM-HIT the prior slot when \
         only an unrelated overlay-set fingerprint moved (augmenter content \
         unchanged) — the overlay augmenter must survive, NOT be dropped by a \
         cold rescan under a fingerprint-keyed slot; got {read2_contributors:?}"
    );
    assert!(
        read2_contributors.contains(&"/aug-base.ts"),
        "warm-hit surface must still include the base augmenter; got {read2_contributors:?}"
    );
}

// ────────────────────────────────────────────────────────────────
// Re-entrant resolver safety — the `ResolvedRelativeCanonical` probe
// resolver runs OFF the `self.artifacts` DashMap guard.
//
// `ensure_augmentation_index_populated`'s cold scan for a
// `ResolvedRelativeCanonical` target invokes the caller's resolver to
// turn a relative `declare module "./x"` specifier into a canonical.
// The production resolver
// (`owner_has_module_augmentation_dependency`'s
// `resolve_type_dependency_canonical`) reaches `ensure_indexed_ready`,
// which materialises the dependency and INSERTS it into the same
// `self.artifacts` DashMap the cold scan iterates.
//
// `DashMap` shards are non-reentrant `std::sync::RwLock`s, so an insert
// into a shard the current thread already read-locks via an active
// `iter()` guard would block on itself (a hang) — or, on a re-entrant
// same-shard access dashmap detects, panic. This test seeds the store
// with multiple base artifacts and a resolver closure that re-enters
// the store with a real `insert_artifacts` write WHILE the probe runs,
// then asserts the call COMPLETES and returns the correct augmenter
// set.
//
// dashmap 6.2's `iter()` holds a read guard on the shard it is
// currently walking, and every yielded `RefMulti` keeps that guard
// alive while the loop body borrows it. A re-entrant `insert` that
// hashes to the SAME shard takes that shard's write lock — a deadlock
// on the non-reentrant per-shard `RwLock`. To make the discriminator
// RELIABLE rather than probabilistic, the resolver below writes enough
// distinct keys to cover every shard, so one write is guaranteed to
// hit the shard the cold-scan iterator holds when the resolver fires.
// Pre-fix (resolver inside `self.artifacts.iter()`) this deadlocks;
// post-fix the snapshot drops every guard before the resolver runs, so
// the call completes. A timeout-style hang assertion is unsafe (a true
// deadlock would hang `cargo test`), so the discriminator is the call
// COMPLETING and returning the correct set.
// ────────────────────────────────────────────────────────────────

#[test]
#[allow(clippy::needless_borrows_for_generic_args)]
fn relative_augmenter_resolver_runs_off_artifacts_guard() {
    let store = FileArtifactStore::new();
    let augmenter_canonical = "/dir/aug-relative.ts";

    // Seed the relative augmenter (`declare module "./local"`).
    let _aug_key = insert_artifact_from_fixture(
        &store,
        augmenter_canonical,
        "module_augmentation_relative.ts",
        [70u8; 16],
    );

    // Seed several additional base artifacts so the cold scan walks
    // multiple DashMap shards before reaching the augmenter.
    for i in 0..8u8 {
        insert_artifact_with_raw_source(
            &store,
            &format!("/dir/filler-{i}.ts"),
            "export const x = 1;\nexport {};\n",
            [80u8 + i; 16],
        );
    }

    let resolved_target = Arc::<str>::from("/dir/local.ts");
    let target = AugmentationTargetKind::ResolvedRelativeCanonical(Arc::clone(&resolved_target));
    let target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target,
    };

    // A reusable payload for the re-entrant writes — cloning the `Arc`
    // is cheap, so the resolver can write many keys without re-parsing.
    let reentrant_payload =
        build_filler_artifacts("export const y = 2;\nexport {};\n", [200u8; 16]);

    // The resolver re-enters the SAME `self.artifacts` DashMap with real
    // writes — the exact hazard `ensure_indexed_ready ->
    // artifacts.insert` poses in production. It writes enough distinct
    // keys to span every shard, so one write is guaranteed to target the
    // shard the cold-scan iterator holds when the resolver fires.
    // Pre-fix that write deadlocks on the held shard guard; post-fix the
    // snapshot has already dropped every guard.
    let resolver_invoked = AtomicBool::new(false);
    let reentrant_inserts = AtomicU32::new(0);
    let resolver = |augmenter: &str, specifier: &str| -> Option<Arc<str>> {
        resolver_invoked.store(true, Ordering::SeqCst);
        let n = reentrant_inserts.fetch_add(1, Ordering::SeqCst);
        // 1024 distinct keys >> any realistic dashmap shard count
        // (default `4 * ncpu` rounded to a power of two), so every shard
        // — including the one the iterator currently read-locks — is
        // written. Reuses one payload `Arc` (no per-key re-parse).
        for j in 0..1024u32 {
            let key = FileArtifactKey {
                canonical: Arc::from(format!("/dir/reentrant-{n}-{j}.ts").as_str()),
                content_hash: {
                    let mut h = [0u8; 16];
                    h[0..4].copy_from_slice(&j.to_le_bytes());
                    h[4] = n as u8;
                    h
                },
                parse_env_hash: [0u8; 16],
                parser_version: 1,
            };
            store.insert_artifacts(key, Arc::clone(&reentrant_payload));
        }
        if augmenter == augmenter_canonical && specifier == "./local" {
            Some(Arc::<str>::from("/dir/local.ts"))
        } else {
            None
        }
    };

    // The discriminator: this call RETURNS (no hang, no re-entrant-write
    // panic) because the snapshot makes the resolver run off the guard.
    let set = store.ensure_augmentation_index_populated(&target_key, &resolver, None);

    assert!(
        resolver_invoked.load(Ordering::SeqCst),
        "the ResolvedRelativeCanonical probe MUST invoke the resolver"
    );
    assert!(
        reentrant_inserts.load(Ordering::SeqCst) >= 1,
        "the resolver MUST have performed at least one re-entrant \
         same-map write (the production hazard this test exercises)"
    );
    assert!(
        !set.entries.is_empty(),
        "the relative augmenter's `./local` specifier resolves to the \
         queried canonical, so the augmenter set MUST be non-empty"
    );
    assert!(
        set.entries
            .iter()
            .any(|e| e.canonical().as_ref() == augmenter_canonical),
        "the matched augmenter MUST be the relative augmenter file"
    );

    // NEGATIVE: a target whose canonical does NOT match the resolver's
    // output yields an EMPTY set (the resolver returns a non-matching
    // canonical for `./local`, so no augmenter contributes).
    let non_matching_target = AugmentationTargetKind::ResolvedRelativeCanonical(Arc::<str>::from(
        "/dir/some-other-module.ts",
    ));
    let non_matching_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: non_matching_target,
    };
    let empty_set = store.ensure_augmentation_index_populated(&non_matching_key, &resolver, None);
    assert!(
        empty_set.entries.is_empty(),
        "a ResolvedRelativeCanonical target whose canonical does NOT \
         match the augmenter's resolved relative specifier MUST yield \
         an empty augmenter set"
    );
}
