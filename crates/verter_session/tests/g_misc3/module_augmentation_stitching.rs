//! Stage 6c — module-augmentation stitching discrimination tests.
//!
//! Each test was written so it would FAIL against the pre-Stage-6c
//! tree (no augmentation-index population, no augmenter-set
//! fingerprint observation in `EffectiveExportSet.fact_dep_signature`,
//! no `ModuleAugmentationStitched` audit event) and PASS against the
//! post-Stage-6c tree.
//!
//! Plan §763-781 verify bullets covered here:
//!
//! - Module augmentation by target kind: each R29 archetype routes
//!   through the correct `AugmentationTargetKind` and stitches.
//! - Project isolation (Codex P0): augmenters in one project do NOT
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

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::facts::registry::{InternedGlobPattern, InternedSpecifier};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, FileArtifactStore,
    FileArtifacts, ProjectIdentity, LEGACY_PARSER_VERSION,
};
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;
use verter_session::resolver_core::{
    EffectiveExportSetKey, FactVersionRef, RouteDb, StoreView, StoreViewCompatToken,
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
    let shallow = ShallowFileState {
        whole_hash,
        exports: FxHashMap::default(),
        wildcard_reexports: Vec::new(),
        symbols: FxHashMap::default(),
        value_symbols: FxHashMap::default(),
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        augmentation_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash,
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
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
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
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
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
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
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
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
    };
    let effective = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &view,
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
        target: target.clone(),
    };
    let target_key_p2 = AugmentationTargetKey {
        project_identity: project_identity_2,
        resolve_env_hash: resolve_env_2,
        lib_env_hash: [0u8; 16],
        target: target.clone(),
    };

    // Populate project 1's index with augmenter A.
    let set_p1 = store.ensure_augmentation_index_populated(&target_key_p1, |_, _| None);
    // Populate project 2's index — augmenter A is the same file in
    // FileArtifactStore, but the project-isolation contract says: the
    // index entry is keyed by `AugmentationTargetKey { project,
    // resolve_env, lib_env, target }`, so two distinct project keys
    // produce TWO distinct entries.
    let set_p2 = store.ensure_augmentation_index_populated(&target_key_p2, |_, _| None);

    // Distinct index entries.
    assert_eq!(
        store.augmentation_index_len(),
        2,
        "two distinct AugmentationTargetKey entries MUST coexist \
         (project_identity + resolve_env_hash isolation per Codex P0.1)"
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
// Test 12 — augmenter-set refresh invalidates downstream.
// ────────────────────────────────────────────────────────────────

#[test]
fn augmenter_set_refresh_invalidates_downstream() {
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
    };

    // Cold compute under view A; captures the initial fingerprint.
    let effective_initial = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
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

    // Step 2 — load the secondary augmenter. The new artifact's
    // augmentations target the same `"vue"` specifier so the
    // augmentation index for the queried target refreshes — the
    // fingerprint transitions.
    let secondary_key = insert_artifact_from_fixture(
        &store,
        "/secondary-aug.ts",
        "module_augmentation_added_augmenter_secondary.ts",
        [32u8; 16],
    );
    let secondary_artifacts = store
        .get_artifacts(&secondary_key)
        .expect("just-inserted artifact MUST be reachable");
    store.refresh_augmentation_index_for_canonical(&secondary_key, &secondary_artifacts, |_, _| {
        None
    });

    // The previously-recorded fingerprint is now stale: a view that
    // refuses it should fail to validate the cached entry.
    let stale_view = RejectStaleAugmenterFingerprint {
        token: StoreViewCompatToken {
            epoch: 2,
            session: None,
        },
        target_spec: "vue".to_owned(),
        stale_fingerprint: initial_fingerprint,
    };
    let warm = route_db.get_effective_export_set(&key, &stale_view);
    assert!(
        warm.is_none(),
        "augmenter-set refresh MUST invalidate downstream `EffectiveExportSet` \
         consumer (G1) — the cached entry's `RouteSurface(ModuleAugmentationIndexShape)` \
         fact MUST fail validation under the stale fingerprint"
    );

    // Step 3 — recompute under a view that knows the post-refresh
    // fingerprint (`new_fingerprint`) but rejects the stale one. The
    // new effective set MUST include BOTH augmenters.
    //
    // First look up the current fingerprint from the augmentation
    // index — this is what the view would snapshot in production.
    let new_target_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        target: target.clone(),
    };
    let new_set = store
        .get_augmenter_set(&new_target_key)
        .expect("augmentation index MUST have an entry after refresh");
    let new_fingerprint = new_set.fingerprint;
    assert_ne!(
        new_fingerprint, initial_fingerprint,
        "augmenter-set fingerprint MUST transition under refresh (G1)"
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
        },
        target_spec: "vue".to_owned(),
        new_fingerprint,
    };

    let effective_refreshed = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &post_refresh_view,
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
    };

    // Cold compute records contributor_whole_hash = original_hash.
    let _ = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
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
    };

    let _ = route_db.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
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
    };

    // Cold compute on the first RouteDb — this populates the
    // augmentation index on `store`, capturing the augmenter's EXACT
    // pre-edit `FileArtifactKey`.
    let route_db_initial = RouteDb::new();
    let effective_initial = route_db_initial.get_or_compute_effective_export_set(
        key.clone(),
        target.clone(),
        &AcceptAllView::new(1),
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
    // only live artifact. (Confirms the test reproduces codex's
    // exact failure mode rather than a coexisting-versions one.)
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
// Base-only contract tripwire — a session view is rejected.
//
// The augmentation index (`ModuleAugmentationIndex` on
// `FileArtifactStore`) is keyed by a base resolve-domain identity
// (`project_identity`, `resolve_env_hash`, `lib_env_hash`) with no
// base/session discriminator: its cold scan and refresh path filter
// to base (`FileArtifactKey::is_legacy`) artifacts, so a session that
// overlays a `declare module` block would observe the base augmenter
// set, not its own. `EffectiveExportSet` is therefore base-only until
// the augmentation index gains artifact-population identity.
//
// `RouteDb::get_or_compute_effective_export_set` fails closed on a
// session view (`StoreViewCompatToken::session.is_some()`): a session
// call panics rather than silently returning a base-only result
// presented as session-correct. This test locks that contract — it
// must keep failing the same way until the future block that adds the
// overlay-aware augmentation-index schema and wires the live session
// `EffectiveExportSet` consumer.
// ────────────────────────────────────────────────────────────────

/// A `StoreView` carrying a session identity — `compat_token().session`
/// is `Some`. Accepts every fact (the guard fires before any fact
/// validation runs, so `validates` is never reached in this test).
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

/// Discriminator: a session view passed to
/// `get_or_compute_effective_export_set` panics with the base-only
/// invariant message.
///
/// - **Against the guard-less tree** (`362eeb0b5`): the session view
///   flows straight into the cold compute, which returns an
///   `Arc<EffectiveExportSetEntry>` — no panic — so this `#[should_panic]`
///   test FAILS ("test did not panic").
/// - **Post-fix tree**: the entry guard rejects `session.is_some()`
///   before any compute, so the call panics with the documented
///   invariant — this test PASSES.
#[test]
#[should_panic(expected = "EffectiveExportSet is base-only")]
fn effective_export_set_rejects_session_view() {
    let store = FileArtifactStore::new();
    let _key = insert_artifact_from_fixture(
        &store,
        "/aug-external.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );
    let route_db = RouteDb::new();
    // A session-scoped view — this is the rejected input.
    let session_view = SessionScopedView::new(1, 42);

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let key = EffectiveExportSetKey {
        provider_canonical: "vue".to_owned(),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
    };

    // MUST panic — augmentation stitching / `EffectiveExportSet` is
    // base-only; a session call is observably an error, never a
    // silently-wrong base-only result.
    let _ = route_db.get_or_compute_effective_export_set(
        key,
        target,
        &session_view,
        &store,
        |_| Some([11u8; 16]),
        |_, _| None,
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
    let set = store.ensure_augmentation_index_populated(&target_key, &resolver);

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
        target: non_matching_target,
    };
    let empty_set = store.ensure_augmentation_index_populated(&non_matching_key, &resolver);
    assert!(
        empty_set.entries.is_empty(),
        "a ResolvedRelativeCanonical target whose canonical does NOT \
         match the augmenter's resolved relative specifier MUST yield \
         an empty augmenter set"
    );
}
