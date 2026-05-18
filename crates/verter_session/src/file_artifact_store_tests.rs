//! `FileArtifactStore` unit tests.

use std::sync::Arc;

use verter_semantic::analysis::Hash16;

use super::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, FileArtifactStore,
    FileArtifacts, ProjectIdentity,
};
use crate::project_type_store::IndexedReady;

fn synth_indexed(hash: u8) -> Arc<IndexedReady> {
    Arc::new(IndexedReady::new_for_test([hash; 16]))
}

fn synth_artifacts(hash: u8) -> Arc<FileArtifacts> {
    Arc::new(FileArtifacts::with_indexed(synth_indexed(hash)))
}

fn synth_key(canonical: &str, content_hash: Hash16, parse_env_hash: Hash16) -> FileArtifactKey {
    FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash,
        parser_version: 1,
    }
}

#[test]
fn empty_store_returns_none() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [0u8; 16], [1u8; 16]);
    assert!(store.get_artifacts(&key).is_none());
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
}

#[test]
fn insert_then_get_returns_payload() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [1u8; 16], [2u8; 16]);
    let payload = synth_artifacts(0xaa);
    store.insert_artifacts(key.clone(), Arc::clone(&payload));
    let got = store.get_artifacts(&key).expect("entry MUST exist");
    assert!(Arc::ptr_eq(&got, &payload), "MUST return the inserted Arc");
    assert_eq!(store.len(), 1);
}

#[test]
fn two_content_hashes_for_same_canonical_coexist() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    assert!(store.get_artifacts(&key_a).is_some());
    assert!(store.get_artifacts(&key_b).is_some());
    assert_eq!(
        store.len(),
        2,
        "two content hashes MUST coexist under same canonical"
    );
}

#[test]
fn two_parse_envs_for_same_canonical_coexist() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [9u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [9u8; 16], [11u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    assert_eq!(
        store.len(),
        2,
        "two parse envs MUST coexist under same (canonical, content_hash)"
    );
    assert!(store.get_artifacts(&key_a).is_some());
    assert!(store.get_artifacts(&key_b).is_some());
}

#[test]
fn remove_artifacts_returns_previous_entry() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [0u8; 16], [1u8; 16]);
    store.insert_artifacts(key.clone(), synth_artifacts(0xcc));
    let removed = store.remove_artifacts(&key);
    assert!(removed.is_some(), "remove MUST return prior entry");
    assert!(
        store.get_artifacts(&key).is_none(),
        "post-remove get MUST be None"
    );
}

#[test]
fn remove_canonical_drops_every_version() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    let key_other = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a, synth_artifacts(0xaa));
    store.insert_artifacts(key_b, synth_artifacts(0xbb));
    store.insert_artifacts(key_other.clone(), synth_artifacts(0xcc));
    let removed = store.remove_canonical("/a.ts");
    assert_eq!(removed, 2, "MUST drop both versions of /a.ts");
    assert!(
        store.get_artifacts(&key_other).is_some(),
        "MUST NOT touch /b.ts"
    );
}

#[test]
fn get_artifacts_any_returns_some_entry_for_canonical() {
    let store = FileArtifactStore::new();
    // `get_artifacts_any` is a base canonical-wide scan — it surfaces
    // only `legacy`-key (base) artifacts, never overlay-scoped ones.
    let key = FileArtifactKey::legacy(Arc::from("/a.ts"), [9u8; 16]);
    store.insert_artifacts(key, synth_artifacts(0xaa));
    assert!(store.get_artifacts_any("/a.ts").is_some());
    assert!(store.get_artifacts_any("/nonexistent.ts").is_none());
}

#[test]
fn augmentation_index_starts_empty() {
    let store = FileArtifactStore::new();
    assert_eq!(store.augmentation_index_len(), 0);
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([0u8; 16]),
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    assert!(store.get_augmenter_set(&key).is_none());
}

#[test]
fn augmentation_index_round_trip() {
    use smallvec::smallvec;

    use super::AugmenterSet;

    let store = FileArtifactStore::new();
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([42u8; 16]),
        resolve_env_hash: [1u8; 16],
        lib_env_hash: [2u8; 16],
        target: AugmentationTargetKind::ExternalSpecifier(super::InternedSpecifier::from("vue")),
    };
    let set = Arc::new(AugmenterSet {
        entries: smallvec![(Arc::from("/aug.ts"), [3u8; 16])],
        fingerprint: [4u8; 16],
    });
    store.populate_augmenter_set(key.clone(), Arc::clone(&set));
    let got = store.get_augmenter_set(&key).expect("MUST round-trip");
    assert!(Arc::ptr_eq(&got, &set));
    assert_eq!(store.augmentation_index_len(), 1);
}

#[test]
fn snapshot_artifacts_observes_every_entry() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a, synth_artifacts(0xaa));
    store.insert_artifacts(key_b, synth_artifacts(0xbb));
    let snap = store.snapshot_artifacts();
    assert_eq!(snap.len(), 2);
}

#[test]
fn artifact_keys_returns_every_key() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert_artifacts(key_a.clone(), synth_artifacts(0xaa));
    store.insert_artifacts(key_b.clone(), synth_artifacts(0xbb));
    let keys = store.artifact_keys();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&key_a));
    assert!(keys.contains(&key_b));
}

// ── Legacy API smoke ──

#[test]
fn legacy_insert_get_round_trip() {
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy.ts");
    let indexed = Arc::new(IndexedReady::new_for_test([7u8; 16]));
    store.insert(Arc::clone(&canonical), Arc::clone(&indexed));
    let got = store.get("/legacy.ts", [7u8; 16]).expect("MUST hit");
    assert!(Arc::ptr_eq(&got, &indexed));
    assert_eq!(store.len(), 1);
    // get_any without hash lookup also succeeds.
    let any = store.get_any("/legacy.ts").expect("MUST hit");
    assert!(Arc::ptr_eq(&any, &indexed));
}

#[test]
fn legacy_remove_drops_entry() {
    let store = FileArtifactStore::new();
    let canonical: Arc<str> = Arc::from("/legacy.ts");
    let indexed = Arc::new(IndexedReady::new_for_test([7u8; 16]));
    store.insert(Arc::clone(&canonical), indexed);
    store.remove("/legacy.ts");
    assert!(store.get("/legacy.ts", [7u8; 16]).is_none());
    assert!(store.is_empty());
}

// ── Overlay-scoped isolation from base canonical-wide scans ──
//
// Block 2.S-F fix-round-1 introduced [`FileArtifactKey::overlay_scoped`]
// so a session-view overlay artifact is keyed distinctly from the base
// artifact (the discriminator lives in the `parse_env_hash` dimension).
// Exact-key lookups (`get` / `get_overlay_scoped` / `get_artifacts`)
// are isolated by the key. The canonical-wide *scans*
// (`content_hash_for_canonical`, `get_any`, `get_artifacts_any`,
// `snapshot_all`) match by `canonical` only, so fix-round-1 alone left
// them able to surface an overlay-scoped artifact to a base reader —
// which would then derive base cache keys / route facts from
// session-specific import routes. These tests pin the completed
// isolation: a base canonical-wide scan must NEVER surface an
// overlay-scoped artifact.

/// Stable non-zero overlay discriminator for the isolation tests.
/// Mirrors the `parse_env_hash` shape `FileArtifactKey::overlay_scoped`
/// builds from a session view's overlay-set fingerprint — non-zero so
/// it can never alias [`super::LEGACY_PARSE_ENV_HASH`].
fn overlay_discriminator_for_test() -> Hash16 {
    [
        b'v', b'o', b'v', b'l', b'-', b'a', b'r', b't', 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88,
    ]
}

#[test]
fn base_canonical_wide_scans_do_not_surface_overlay_only_artifact() {
    // Discrimination property: a session installs an overlay for
    // canonical X and the overlay artifact is published under its
    // `overlay_scoped` key — and NO base (`legacy`-key) artifact
    // exists for X. A *base* scan for X must therefore return `None`
    // (a base reader sees no base artifact), NEVER the overlay-scoped
    // artifact.
    //
    // Pre-fix (`7c1c0429a`): `get_any` / `get_artifacts_any` /
    // `content_hash_for_canonical` / `snapshot_all` match `canonical`
    // only, so the overlay-scoped entry — the sole entry for X — is
    // surfaced to the base reader. Post-fix: the scans filter to
    // `legacy` keys and return `None` / omit X.
    let store = FileArtifactStore::new();
    let content_hash = [0x5au8; 16];
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/overlay-only.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x5a));

    // A base canonical-wide scan must NOT surface the overlay artifact.
    assert!(
        store.get_any("/overlay-only.ts").is_none(),
        "get_any (base scan) MUST NOT surface an overlay-scoped artifact"
    );
    assert!(
        store.get_artifacts_any("/overlay-only.ts").is_none(),
        "get_artifacts_any (base scan) MUST NOT surface an overlay-scoped artifact"
    );
    assert!(
        store
            .content_hash_for_canonical("/overlay-only.ts")
            .is_none(),
        "content_hash_for_canonical (base scan) MUST NOT surface an overlay-scoped artifact's hash"
    );
    assert!(
        store
            .snapshot_all()
            .iter()
            .all(|(canonical, _)| canonical.as_ref() != "/overlay-only.ts"),
        "snapshot_all (base scan) MUST NOT include an overlay-scoped artifact"
    );

    // Inverse: the view-aware exact-key accessor still reaches it.
    assert!(
        store
            .get_overlay_scoped(
                "/overlay-only.ts",
                content_hash,
                overlay_discriminator_for_test()
            )
            .is_some(),
        "get_overlay_scoped (view-aware accessor) MUST still reach the overlay artifact"
    );
}

#[test]
fn base_canonical_wide_scans_return_base_artifact_when_base_and_overlay_coexist() {
    // Discrimination property: a base artifact and an overlay-scoped
    // artifact for the SAME canonical + content hash coexist (the
    // byte-identical-overlay case — the common LSP case). A base scan
    // MUST return exactly the base artifact, never the overlay one.
    //
    // Pre-fix: the scan matches `canonical` only and DashMap iteration
    // order decides which of the two entries is surfaced — the overlay
    // artifact can win. Post-fix: the scan filters to the `legacy` key
    // and deterministically returns the base artifact.
    let store = FileArtifactStore::new();
    let content_hash = [0x77u8; 16];
    let base_indexed = synth_indexed(0xb0);
    let base_key = FileArtifactKey::legacy(Arc::from("/shared.ts"), content_hash);
    store.insert_artifacts(
        base_key,
        Arc::new(FileArtifacts::with_indexed(Arc::clone(&base_indexed))),
    );
    let overlay_indexed = synth_indexed(0x0e);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/shared.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(
        overlay_key,
        Arc::new(FileArtifacts::with_indexed(Arc::clone(&overlay_indexed))),
    );

    let any = store
        .get_any("/shared.ts")
        .expect("get_any MUST hit the base artifact");
    assert!(
        Arc::ptr_eq(&any, &base_indexed),
        "get_any MUST return the base artifact, never the overlay-scoped sibling"
    );
    let any_artifacts = store
        .get_artifacts_any("/shared.ts")
        .expect("get_artifacts_any MUST hit the base artifact");
    assert!(
        Arc::ptr_eq(&any_artifacts.indexed, &base_indexed),
        "get_artifacts_any MUST return the base artifact, never the overlay-scoped sibling"
    );
    let snap = store.snapshot_all();
    let shared_entries: Vec<&Arc<IndexedReady>> = snap
        .iter()
        .filter(|(canonical, _)| canonical.as_ref() == "/shared.ts")
        .map(|(_, indexed)| indexed)
        .collect();
    assert_eq!(
        shared_entries.len(),
        1,
        "snapshot_all MUST surface exactly one entry for the canonical (the base)"
    );
    assert!(
        Arc::ptr_eq(shared_entries[0], &base_indexed),
        "snapshot_all MUST surface the base artifact, never the overlay-scoped sibling"
    );

    // The overlay-scoped exact-key read still reaches the overlay.
    let overlay_hit = store
        .get_overlay_scoped("/shared.ts", content_hash, overlay_discriminator_for_test())
        .expect("get_overlay_scoped MUST still reach the overlay artifact");
    assert!(
        Arc::ptr_eq(&overlay_hit, &overlay_indexed),
        "get_overlay_scoped MUST reach the overlay artifact"
    );
}

#[test]
fn get_artifacts_for_content_stays_view_independent_across_base_and_overlay() {
    // `get_artifacts_for_content` is content-addressed and
    // view-independent BY DESIGN: its sole consumer
    // (`parse_fact_ref_for_observed_current_content`) reads the
    // parse-domain `FileFacts` registry, which is derived purely from
    // the source bytes — identical across a base artifact and a
    // byte-identical overlay artifact at the same content hash. It
    // MUST therefore still resolve a `FileArtifacts` for a content
    // version that exists ONLY as an overlay-scoped artifact, so a
    // parse fact can still be recovered.
    let store = FileArtifactStore::new();
    let content_hash = [0x3cu8; 16];
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/overlay-only.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(overlay_key, synth_artifacts(0x3c));
    assert!(
        store
            .get_artifacts_for_content("/overlay-only.ts", content_hash)
            .is_some(),
        "get_artifacts_for_content MUST stay content-addressed / view-independent \
         so parse-fact recovery works for an overlay-only content version"
    );
    // A mismatched content hash still misses (content-pinned).
    assert!(
        store
            .get_artifacts_for_content("/overlay-only.ts", [0x00u8; 16])
            .is_none(),
        "get_artifacts_for_content MUST stay content-pinned"
    );
}

#[test]
fn remove_canonical_drains_overlay_scoped_keys() {
    // A removal / eviction scan MUST keep draining ALL of a
    // canonical's keys — base AND overlay-scoped — so an eviction
    // never leaves a stale overlay artifact behind. `remove_canonical`
    // is a lifecycle scan, NOT a base-read scan: it stays unfiltered.
    let store = FileArtifactStore::new();
    let content_hash = [0x9bu8; 16];
    let base_key = FileArtifactKey::legacy(Arc::from("/evict-me.ts"), content_hash);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/evict-me.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(base_key.clone(), synth_artifacts(0xb0));
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x0e));
    assert_eq!(
        store.len(),
        2,
        "both base + overlay entries MUST be present"
    );

    let removed = store.remove_canonical("/evict-me.ts");
    assert_eq!(
        removed, 2,
        "remove_canonical MUST drain BOTH the base and overlay-scoped keys"
    );
    assert_eq!(store.len(), 0, "no entry MUST survive the eviction");
    assert!(
        store.get_artifacts(&overlay_key).is_none(),
        "the overlay-scoped artifact MUST NOT survive remove_canonical"
    );
    assert!(
        store.get_artifacts(&base_key).is_none(),
        "the base artifact MUST NOT survive remove_canonical"
    );
}

#[test]
fn legacy_remove_drains_overlay_scoped_keys() {
    // `remove` (the legacy per-canonical removal) is likewise a
    // lifecycle scan and MUST drain overlay-scoped keys too.
    let store = FileArtifactStore::new();
    let content_hash = [0xa5u8; 16];
    let base_key = FileArtifactKey::legacy(Arc::from("/drop-me.ts"), content_hash);
    let overlay_key = FileArtifactKey::overlay_scoped(
        Arc::from("/drop-me.ts"),
        content_hash,
        overlay_discriminator_for_test(),
    );
    store.insert_artifacts(base_key.clone(), synth_artifacts(0xb0));
    store.insert_artifacts(overlay_key.clone(), synth_artifacts(0x0e));

    store.remove("/drop-me.ts");
    assert!(
        store.is_empty(),
        "remove MUST drain base + overlay-scoped keys"
    );
    assert!(store.get_artifacts(&overlay_key).is_none());
    assert!(store.get_artifacts(&base_key).is_none());
}
