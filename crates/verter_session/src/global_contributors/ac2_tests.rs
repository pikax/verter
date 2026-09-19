//! Coherence of global contributor snapshots under concurrent publication.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{ContributorOrigin, GlobalContributorIndex};
use crate::file_artifact_store::{
    AugmentationTargetKind, FileArtifactKey, FileArtifactStore, FileArtifacts, BASE_PARSE_ENV_HASH,
};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::ShallowFileState;

fn publish(store: &FileArtifactStore, canonical: &str, source: &str) {
    let state = ShallowFileState::service_backed_for_test_at(canonical, source);
    let hash = state.whole_hash;
    let src: Arc<str> = Arc::from(source);
    let indexed = Arc::new(IndexedReady::new_for_test_with_state(
        hash,
        state,
        Arc::clone(&src),
        src,
    ));
    store.insert(Arc::from(canonical), indexed);
}

fn fingerprint(store: &FileArtifactStore, name: &str) -> verter_semantic::analysis::Hash16 {
    store
        .global_contributor_index()
        .snapshot()
        .lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            name,
            None,
            true,
        )
        .fingerprint
}

#[test]
fn forward_reverse_random_ingest_yield_identical_precedence() {
    let sources = [
        (
            "/a.d.ts",
            "export {};\ndeclare global { interface Window { a: 1 } }\n",
        ),
        (
            "/b.d.ts",
            "export {};\ndeclare global { interface Window { b: 2 } }\n",
        ),
        (
            "/c.d.ts",
            "export {};\ndeclare global { interface Window { c: 3 } }\n",
        ),
    ];

    let forward = FileArtifactStore::new();
    for (canonical, source) in sources {
        publish(&forward, canonical, source);
    }
    let reverse = FileArtifactStore::new();
    for (canonical, source) in sources.iter().rev() {
        publish(&reverse, canonical, source);
    }
    let random = FileArtifactStore::new();
    for idx in [1, 0, 2] {
        publish(&random, sources[idx].0, sources[idx].1);
    }

    let names = |store: &FileArtifactStore| {
        store
            .global_contributor_index()
            .snapshot()
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                "Window",
                None,
                true,
            )
            .entries
            .iter()
            .map(|entry| entry.artifact_key.canonical.to_string())
            .collect::<Vec<_>>()
    };
    let forward_names = names(&forward);
    assert_eq!(forward_names.len(), 3);
    assert_eq!(forward_names, names(&reverse));
    assert_eq!(forward_names, names(&random));
    assert_eq!(
        fingerprint(&forward, "Window"),
        fingerprint(&reverse, "Window")
    );
    assert_eq!(
        fingerprint(&forward, "Window"),
        fingerprint(&random, "Window")
    );
}

#[test]
fn concurrent_inserts_publish_a_coherent_snapshot() {
    let store = Arc::new(FileArtifactStore::new());
    std::thread::scope(|scope| {
        for i in 0..8 {
            let store = Arc::clone(&store);
            scope.spawn(move || {
                let canonical = format!("/g{i}.d.ts");
                let source = format!(
                    "export {{}};\ndeclare global {{ interface Window {{ f{i}: {i} }} }}\n"
                );
                publish(&store, &canonical, &source);
            });
        }
    });
    let snap = store.global_contributor_index().snapshot();
    let window = snap.lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "Window",
        None,
        true,
    );
    assert_eq!(window.entries.len(), 8);
    assert!(window
        .entries
        .iter()
        .all(|entry| entry.origin == ContributorOrigin::DeclareGlobal));
    let again = store.global_contributor_index().snapshot();
    assert_eq!(
        snap.lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Window",
            None,
            true,
        )
        .fingerprint,
        again
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                "Window",
                None,
                true,
            )
            .fingerprint,
        "readers of a published snapshot must agree with a subsequent coherent snapshot of the same membership"
    );
}

#[test]
fn racing_reader_sees_old_or_new_never_a_partial_set() {
    let store = Arc::new(FileArtifactStore::new());
    publish(
        &store,
        "/a.d.ts",
        "export {};\ndeclare global { interface Window { a: 1 } }\n",
    );
    let before = fingerprint(&store, "Window");
    std::thread::scope(|scope| {
        let reader = Arc::clone(&store);
        scope.spawn(move || {
            for _ in 0..64 {
                // bounded-loop: snapshot coherence probe
                let got = reader.global_contributor_index().snapshot().lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "Window",
                    None,
                    true,
                );
                assert!(
                    got.entries.len() == 1 || got.entries.len() == 2,
                    "a reader must see the old or new coherent set, got {}",
                    got.entries.len()
                );
            }
        });
        publish(
            &store,
            "/b.d.ts",
            "export {};\ndeclare global { interface Window { b: 2 } }\n",
        );
    });
    let after = fingerprint(&store, "Window");
    assert_ne!(before, after);
    assert_eq!(
        store
            .global_contributor_index()
            .snapshot()
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                "Window",
                None,
                true,
            )
            .entries
            .len(),
        2
    );
}

#[test]
fn publication_pins_epoch_before_reading_membership() {
    let store = FileArtifactStore::new();
    let index = store.global_contributor_index();
    publish(
        &store,
        "/a.d.ts",
        "export {};\ndeclare global { interface Window { a: 1 } }\n",
    );
    let before = index.snapshot().revision;
    index.publish(index.snapshot().program_snapshot, || u64::MAX);
    assert_eq!(
        index.snapshot().revision,
        before,
        "a publication must abandon when the live epoch moved after the pin"
    );
}

fn note_live_window(index: &GlobalContributorIndex) {
    let source = "export {};\ndeclare global { interface Window { x: 1 } }\n";
    let state = ShallowFileState::service_backed_for_test_at("/g.d.ts", source);
    let src: Arc<str> = Arc::from(source);
    let indexed =
        IndexedReady::new_for_test_with_state(state.whole_hash, state, Arc::clone(&src), src);
    let key = FileArtifactKey::for_indexed(Arc::from("/g.d.ts"), &indexed, BASE_PARSE_ENV_HASH);
    let payload = FileArtifacts::with_indexed(Arc::new(indexed));
    index.note_live(key, &payload);
}

#[test]
fn publish_now_exhaustion_still_publishes_dirty_contributors() {
    let index = GlobalContributorIndex::new();
    note_live_window(&index);
    let ticks = AtomicU64::new(0);
    index.publish_now(|| ticks.fetch_add(1, Ordering::Relaxed) + 1);
    let hit = index.snapshot().lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "Window",
        None,
        true,
    );
    assert!(
        !hit.entries.is_empty(),
        "retry exhaustion must still publish the dirty contributor set"
    );
}

#[test]
fn clear_cannot_publish_a_contributor_missing_from_records() {
    let store = Arc::new(FileArtifactStore::new());
    std::thread::scope(|scope| {
        for t in 0..4 {
            let store = Arc::clone(&store);
            scope.spawn(move || {
                for i in 0..24 {
                    let canonical = format!("/g{t}_{i}.d.ts");
                    publish(
                        &store,
                        &canonical,
                        "export {};\ndeclare global { interface Window { x: 1 } }\n",
                    );
                }
            });
        }
        let store = Arc::clone(&store);
        scope.spawn(move || {
            for _ in 0..48 {
                store.global_contributor_index().clear();
            }
        });
    });
    let index = store.global_contributor_index();
    let records = index.record_canonicals();
    let published = index.snapshot().lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "Window",
        None,
        true,
    );
    for entry in published.entries.iter() {
        assert!(
            records.contains(&entry.artifact_key.canonical),
            "published contributor {} must still exist in records",
            entry.artifact_key.canonical
        );
    }
}
