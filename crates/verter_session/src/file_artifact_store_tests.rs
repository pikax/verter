//! Stage 1 — `FileArtifactStore` unit tests.
//!
//! Discriminating tests for the per-host content-addressed file artifact
//! cache. The integration tests in
//! `crates/verter_session/tests/file_artifact_store_smoke.rs` and
//! `crates/verter_session/tests/cache_key_invariants.rs` cover the
//! cross-module behaviour and the R5/R6 key invariants.

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
    assert!(store.get(&key).is_none());
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
}

#[test]
fn insert_then_get_returns_payload() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [1u8; 16], [2u8; 16]);
    let payload = synth_artifacts(0xaa);
    store.insert(key.clone(), Arc::clone(&payload));
    let got = store.get(&key).expect("entry MUST exist");
    assert!(Arc::ptr_eq(&got, &payload), "MUST return the inserted Arc");
    assert_eq!(store.len(), 1);
}

#[test]
fn two_content_hashes_for_same_canonical_coexist() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    store.insert(key_a.clone(), synth_artifacts(0xaa));
    store.insert(key_b.clone(), synth_artifacts(0xbb));
    assert!(store.get(&key_a).is_some());
    assert!(store.get(&key_b).is_some());
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
    store.insert(key_a.clone(), synth_artifacts(0xaa));
    store.insert(key_b.clone(), synth_artifacts(0xbb));
    assert_eq!(
        store.len(),
        2,
        "two parse envs MUST coexist under same (canonical, content_hash)"
    );
    assert!(store.get(&key_a).is_some());
    assert!(store.get(&key_b).is_some());
}

#[test]
fn remove_returns_previous_entry() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [0u8; 16], [1u8; 16]);
    store.insert(key.clone(), synth_artifacts(0xcc));
    let removed = store.remove(&key);
    assert!(removed.is_some(), "remove MUST return prior entry");
    assert!(store.get(&key).is_none(), "post-remove get MUST be None");
}

#[test]
fn remove_canonical_drops_every_version() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/a.ts", [2u8; 16], [10u8; 16]);
    let key_other = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert(key_a, synth_artifacts(0xaa));
    store.insert(key_b, synth_artifacts(0xbb));
    store.insert(key_other.clone(), synth_artifacts(0xcc));
    let removed = store.remove_canonical("/a.ts");
    assert_eq!(removed, 2, "MUST drop both versions of /a.ts");
    assert!(store.get(&key_other).is_some(), "MUST NOT touch /b.ts");
}

#[test]
fn get_any_returns_some_entry_for_canonical() {
    let store = FileArtifactStore::new();
    let key = synth_key("/a.ts", [9u8; 16], [10u8; 16]);
    store.insert(key, synth_artifacts(0xaa));
    assert!(store.get_any("/a.ts").is_some());
    assert!(store.get_any("/nonexistent.ts").is_none());
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
fn snapshot_all_observes_every_entry() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert(key_a, synth_artifacts(0xaa));
    store.insert(key_b, synth_artifacts(0xbb));
    let snap = store.snapshot_all();
    assert_eq!(snap.len(), 2);
}

#[test]
fn keys_returns_every_key() {
    let store = FileArtifactStore::new();
    let key_a = synth_key("/a.ts", [1u8; 16], [10u8; 16]);
    let key_b = synth_key("/b.ts", [1u8; 16], [10u8; 16]);
    store.insert(key_a.clone(), synth_artifacts(0xaa));
    store.insert(key_b.clone(), synth_artifacts(0xbb));
    let keys = store.keys();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&key_a));
    assert!(keys.contains(&key_b));
}
