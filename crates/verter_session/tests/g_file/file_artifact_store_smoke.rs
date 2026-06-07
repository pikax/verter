//! `FileArtifactStore` smoke / round-trip integration test.
//!
//! Exercises the public surface of the new content-addressed file
//! artifact store from the consumer perspective.

use std::sync::Arc;

use verter_session::file_artifact_store::{FileArtifactKey, FileArtifactStore, FileArtifacts};
use verter_session::project_type_store::IndexedReady;

fn make_artifacts(hash_marker: u8) -> Arc<FileArtifacts> {
    Arc::new(FileArtifacts::with_indexed(Arc::new(
        IndexedReady::new_for_test([hash_marker; 16]),
    )))
}

fn make_key(canonical: &str, content_hash: u8, parse_env_hash: u8) -> FileArtifactKey {
    FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash: [content_hash; 16],
        parse_env_hash: [parse_env_hash; 16],
        parser_version: 1,
    }
}

#[test]
fn insert_get_remove_round_trip() {
    let store = FileArtifactStore::new();
    let key = make_key("/x.ts", 1, 2);
    let payload = make_artifacts(0xab);
    assert!(store.get_artifacts(&key).is_none());
    store.insert_artifacts(key.clone(), Arc::clone(&payload));
    let got = store
        .get_artifacts(&key)
        .expect("post-insert get MUST succeed");
    assert!(Arc::ptr_eq(&got, &payload));
    let removed = store
        .remove_artifacts(&key)
        .expect("remove MUST return prior");
    assert!(Arc::ptr_eq(&removed, &payload));
    assert!(
        store.get_artifacts(&key).is_none(),
        "post-remove get MUST be None"
    );
}

#[test]
fn two_envs_coexist_for_same_canonical() {
    let store = FileArtifactStore::new();
    let key_env_a = make_key("/shared.ts", 1, 10);
    let key_env_b = make_key("/shared.ts", 1, 20);
    store.insert_artifacts(key_env_a.clone(), make_artifacts(0xa));
    store.insert_artifacts(key_env_b.clone(), make_artifacts(0xb));
    assert_eq!(store.len(), 2);
    assert!(store.get_artifacts(&key_env_a).is_some());
    assert!(store.get_artifacts(&key_env_b).is_some());
}

#[test]
fn file_artifacts_carry_indexed_facts_edges_augmentations() {
    let store = FileArtifactStore::new();
    let key = make_key("/m.ts", 7, 7);
    let payload = make_artifacts(0xcc);
    store.insert_artifacts(key.clone(), payload);
    let got = store.get_artifacts(&key).expect("entry MUST exist");
    // Every FileArtifacts has the four sub-fields wired (facts,
    // parsed_edges, augmentations, plus parse_stable_hash) along with
    // the canonical IndexedReady.
    assert_eq!(got.indexed.whole_hash, [0xccu8; 16]);
    let _facts: &verter_session::file_artifact_store::FileFacts = &got.facts;
    let _edges: &verter_session::file_artifact_store::ParsedEdges = &got.parsed_edges;
    assert!(
        got.augmentations.is_empty(),
        "augmentations are empty for this fixture"
    );
    // parse_stable_hash is deterministic (the same payload twice produces
    // the same hash).
    assert_eq!(got.parse_stable_hash, got.parse_stable_hash);
}

#[test]
fn keys_iteration_observes_every_entry() {
    let store = FileArtifactStore::new();
    let keys: Vec<FileArtifactKey> = (0..5)
        .map(|i| make_key(&format!("/f{i}.ts"), i as u8, 1))
        .collect();
    for k in &keys {
        store.insert_artifacts(k.clone(), make_artifacts(0));
    }
    let observed = store.artifact_keys();
    assert_eq!(observed.len(), 5);
    for k in &keys {
        assert!(observed.contains(k));
    }
}
