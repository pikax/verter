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
    let key = synth_key("/a.ts", [9u8; 16], [10u8; 16]);
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
