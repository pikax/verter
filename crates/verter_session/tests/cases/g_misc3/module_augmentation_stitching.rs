//! Module-augmentation index discrimination tests.
//!
//! These tests exercise the artifact-index substrate used by the live semantic
//! augmentation stitcher: project/session isolation, lifecycle invalidation,
//! exact-key self-healing, parser-version filtering, and lock-safe resolver
//! callbacks. Semantic merge behavior is covered through the production
//! `ProjectSemanticDispatch` path in the session unit suites.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use verter_semantic::facts::registry::InternedSpecifier;
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, FileArtifactStore,
    FileArtifacts, ProjectIdentity, CURRENT_PARSER_VERSION,
};
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

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
        .join("cases")
        .join("fixtures")
        .join("path_precise")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn empty_external(
) -> Arc<verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default())
}

fn build_indexed_with_source(
    canonical: &str,
    raw: &str,
    whole_hash: [u8; 16],
) -> Arc<IndexedReady> {
    // Build the shallow inventory through the REAL service-backed
    // construction (parse → header index → lazy decl-body memo) so the typed
    // augmentation inventory (the single source of truth for augmentation
    // facts) is populated and augmenter bodies lower on demand, exactly as
    // production does.
    let shallow = ShallowFileState::service_backed_for_test_with_hash(canonical, raw, whole_hash);
    Arc::new(IndexedReady::new_for_test_with_state(
        whole_hash,
        shallow,
        Arc::from(raw),
        Arc::from(raw),
        empty_external(),
    ))
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
    let indexed = build_indexed_with_source(canonical, &raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let artifacts = Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    });
    let key = FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: [0u8; 16],
        parser_version: CURRENT_PARSER_VERSION,
        file_language_id: FileArtifactKey::derived_file_language_id(canonical),
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
    let indexed = build_indexed_with_source(canonical, raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let artifacts = Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    });
    let key = FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: [0u8; 16],
        parser_version: CURRENT_PARSER_VERSION,
        file_language_id: FileArtifactKey::derived_file_language_id(canonical),
    };
    store.insert_artifacts(key.clone(), artifacts);
    key
}

/// Build an `Arc<FileArtifacts>` from raw source WITHOUT inserting it.
/// Used to pre-build a reusable payload for cheap re-entrant writes in
/// the resolver-off-guard test.
fn build_filler_artifacts(raw: &str, content_hash: [u8; 16]) -> Arc<FileArtifacts> {
    let indexed = build_indexed_with_source("/filler.ts", raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    })
}

// ────────────────────────────────────────────────────────────────
// Project-scoped augmentation-index isolation.
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
// API invalidates the augmentation index, so the downstream semantic
// stitch observes a new fingerprint. Drives the REAL publish path
// (`FileArtifactStore::insert_artifacts`) — NO direct index-refresh
// call — so it characterizes production lifecycle behavior.
// ────────────────────────────────────────────────────────────────

#[test]
fn session_overlay_augmenter_isolated_from_base_index() {
    use verter_session::file_artifact_store::AugmentationPopulation;

    let store = FileArtifactStore::new();

    // Base augmenter (base key) that `declare module "vue" {}` augments.
    let _base_key = insert_artifact_from_fixture(
        &store,
        "/aug-base.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );

    // Session-overlay augmenter: a DIFFERENT file, keyed under a non-base
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
    let indexed = build_indexed_with_source("/aug-overlay.ts", &raw, [99u8; 16]);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let overlay_key = FileArtifactKey {
        canonical: Arc::from("/aug-overlay.ts"),
        content_hash: [99u8; 16],
        parse_env_hash: overlay_discriminator,
        parser_version: CURRENT_PARSER_VERSION,
        file_language_id: FileArtifactKey::derived_file_language_id("/aug-overlay.ts"),
    };
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts {
            indexed,
            facts: Arc::new(emission.facts),
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

fn seed_base_and_overlay_augmenters(
    store: &FileArtifactStore,
    fingerprint: u64,
) -> verter_session::Hash16 {
    let _ = insert_artifact_from_fixture(
        store,
        "/aug-base.ts",
        "module_augmentation_external.ts",
        [11u8; 16],
    );

    let overlay_discriminator =
        verter_session::session_view::overlay_artifact_discriminator_for_fingerprint(fingerprint);
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source("/aug-overlay.ts", &raw, [99u8; 16]);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let overlay_key = FileArtifactKey {
        canonical: Arc::from("/aug-overlay.ts"),
        content_hash: [99u8; 16],
        parse_env_hash: overlay_discriminator,
        parser_version: CURRENT_PARSER_VERSION,
        file_language_id: FileArtifactKey::derived_file_language_id("/aug-overlay.ts"),
    };
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts {
            indexed,
            facts: Arc::new(emission.facts),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }),
    );
    overlay_discriminator
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
/// stitch misses the new base contributor.
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
        build_indexed_with_source("/primary.ts", &primary_raw, [61u8; 16]),
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
        build_indexed_with_source("/secondary.ts", &secondary_raw, [62u8; 16]),
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
// next semantic stitch observes an incomplete contributor surface.
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
    // writes — the exact hazard `ensure_indexed_ready_serve ->
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
                file_language_id: FileArtifactKey::derived_file_language_id(
                    format!("/dir/reentrant-{n}-{j}.ts").as_str(),
                ),
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

// ────────────────────────────────────────────────────────────────
// parser_version invalidation — a base augmenter stamped at a STALE
// parser_version is EXCLUDED from the stitched augmentation surface.
//
// The augmentation-index base scan filters candidates on `key.is_base()`,
// and `is_base()` is `parse_env_hash == BASE_PARSE_ENV_HASH &&
// parser_version == CURRENT_PARSER_VERSION`. So an artifact whose
// `parser_version` is stale (a pre-bump entry that survived in the store)
// is NOT a base candidate and contributes ZERO augmenters, even though its
// `parse_env_hash` is the base sentinel and its source carries a matching
// `declare module "vue" {}`.
//
// - **Against a hypothetical regression** where `is_base()` ignored
//   `parser_version` (folded only `parse_env_hash`): BOTH the
//   current-version augmenter AND the stale-version augmenter would pass the
//   base filter, the scan would return TWO entries, and the
//   `assert_eq!(canonicals, vec!["/aug-current.ts"])` below would FAIL.
// - **Current tree**: the stale-version augmenter is excluded, the scan
//   returns exactly the current-version augmenter — PASSES.
// ────────────────────────────────────────────────────────────────

/// Insert a file artifact stamped with an explicit `parser_version`
/// (every other dimension matches the base helper). Used to plant a
/// stale-parser-version augmenter the base scan must exclude.
fn insert_artifact_at_parser_version(
    store: &FileArtifactStore,
    canonical: &str,
    fixture_name: &str,
    content_hash: [u8; 16],
    parser_version: u32,
) -> FileArtifactKey {
    let raw = fixture(fixture_name);
    let indexed = build_indexed_with_source(canonical, &raw, content_hash);
    let emission = emit_parse_facts(&indexed);
    let parse_stable_hash = verter_session::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let artifacts = Arc::new(FileArtifacts {
        indexed,
        facts: Arc::new(emission.facts),
        parse_stable_hash,
        augmentations: Arc::new(emission.augmentations),
    });
    let key = FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: [0u8; 16],
        parser_version,
        file_language_id: FileArtifactKey::derived_file_language_id(canonical),
    };
    store.insert_artifacts(key.clone(), artifacts);
    key
}

#[test]
fn stale_parser_version_augmenter_excluded_current_version_contributes() {
    // Stamp the stale augmenter one parser version BELOW current
    // (`CURRENT_PARSER_VERSION` is >= 1, so this never underflows — a 0
    // current version would itself fail to compile this subtraction).
    let stale_parser_version = CURRENT_PARSER_VERSION - 1;

    let store = FileArtifactStore::new();

    // (a) current-version augmenter — a true base candidate.
    let current_key = insert_artifact_from_fixture(
        &store,
        "/aug-current.ts",
        "module_augmentation_external.ts",
        [71u8; 16],
    );
    assert_eq!(
        current_key.parser_version, CURRENT_PARSER_VERSION,
        "the current-version augmenter MUST be stamped CURRENT_PARSER_VERSION"
    );

    // (b) stale-version augmenter — same base `parse_env_hash` sentinel,
    // same matching `declare module "vue" {}` source, DIFFERENT canonical,
    // but stamped at the PRIOR parser version. It is NOT `is_base()`.
    let stale_key = insert_artifact_at_parser_version(
        &store,
        "/aug-stale.ts",
        "module_augmentation_external.ts",
        [72u8; 16],
        stale_parser_version,
    );
    assert_ne!(
        stale_key.parser_version, CURRENT_PARSER_VERSION,
        "the stale augmenter MUST carry a non-current parser_version"
    );
    // The two artifacts coexist in the store (the stale one is not drained
    // by the current-version insert — distinct content hash + version key).
    assert!(
        store.get_artifacts(&current_key).is_some(),
        "current-version augmenter MUST be live in the store"
    );
    assert!(
        store.get_artifacts(&stale_key).is_some(),
        "stale-version augmenter MUST also be live in the store (distinct key)"
    );

    let target = AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from("vue"));
    let base_key = AugmentationTargetKey {
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target,
    };

    let base_set = store.ensure_augmentation_index_populated(&base_key, |_, _| None, None);
    let canonicals: Vec<&str> = base_set
        .entries
        .iter()
        .map(|e| e.canonical().as_ref())
        .collect();

    // DISCRIMINATING assertion: ONLY the current-version augmenter
    // contributes. A regression ignoring parser_version would also admit
    // `/aug-stale.ts` and this exact-vector equality would fail.
    assert_eq!(
        canonicals,
        vec!["/aug-current.ts"],
        "the stale-parser-version augmenter MUST be EXCLUDED from the base \
         augmentation surface; only the CURRENT_PARSER_VERSION augmenter \
         contributes"
    );
    assert_eq!(
        base_set.entries.len(),
        1,
        "exactly one augmenter (the current-version one) contributes"
    );
    assert!(
        !canonicals.contains(&"/aug-stale.ts"),
        "the stale-parser-version augmenter contributes ZERO to the stitched \
         surface"
    );
}
