use std::sync::Arc;

use crate::file_artifact_store::AugmentationTargetKind;
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

fn with_configured_ingestion_held(host: &VerterHost, query: impl FnOnce()) {
    use std::sync::mpsc;

    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = parking_lot::Mutex::new(release_rx);
        *host.test_force.configured_ingestion_batch_hook.0.lock() = Some(Arc::new(move || {
            ready_tx.send(()).expect("ingestion batch opened");
            // Dropping the sender releases ingestion on assertion failure too.
            let _ = release_rx.lock().recv();
        }));
        let ingestion = scope.spawn(|| host.ingest_configured_programs());
        ready_rx.recv().expect("configured ingestion is held");
        assert!(
            host.project_type_store()
                .indexed()
                .global_contributor_index()
                .has_pending_overlay_ambient(),
            "configured ingestion holds an open contributor batch"
        );
        query();
        drop(release_tx);
        ingestion.join().expect("configured ingestion completes");
    });
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

#[test]
fn completed_upsert_is_visible_during_configured_ingestion() {
    use verter_type_engine::semantic_query::{ProjectionMode, SemanticNodeData};

    let host = VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[(
            "/project",
            r#"{"compilerOptions":{"noLib":true},"include":["**/*"]}"#,
        )],
    );
    upsert(&host, "/project/use.ts", "export type Result = NewGlobal;");
    with_configured_ingestion_held(&host, || {
        upsert(
            &host,
            "/project/new.d.ts",
            "interface NewGlobal { value: number }",
        );
        let answer = host
            .resolve_named_symbol("/project/use.ts", "Result", Some(ProjectionMode::Expanded))
            .expect("completed upsert resolves during configured ingestion");
        let data = host
            .project_type_store()
            .semantic_graph()
            .node_data(answer.id())
            .unwrap();
        let SemanticNodeData::Object(view) = data.as_ref() else {
            panic!("expected the completed global's object, got {data:?}");
        };
        let names: Vec<_> = view
            .positive_members()
            .iter()
            .map(|member| member.string_name().expect("named member").to_string())
            .collect();
        assert_eq!(names, ["value"]);
    });
}

#[test]
fn configured_ingestion_preserves_clean_global_warm_hits() {
    use verter_type_engine::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[(
            "/project",
            r#"{"compilerOptions":{"noLib":true},"include":["**/*"]}"#,
        )],
    );
    upsert(
        &host,
        "/project/clean.d.ts",
        "interface CleanGlobal { clean: number }",
    );
    upsert(
        &host,
        "/project/use.ts",
        "export type Result = CleanGlobal;",
    );
    let resolve = || {
        host.resolve_named_symbol("/project/use.ts", "Result", Some(ProjectionMode::Expanded))
            .expect("clean global resolves")
    };
    let cold = resolve();
    let graph = host.project_type_store().semantic_graph();
    let before = graph.stats_snapshot();
    assert_eq!(resolve().id(), cold.id());
    let warm = graph.stats_snapshot();
    assert_eq!(warm.misses, before.misses, "settled control builds nothing");
    assert!(warm.hits > before.hits, "settled control hits the memo");

    with_configured_ingestion_held(&host, || {
        upsert(
            &host,
            "/project/unrelated.d.ts",
            "interface UnrelatedGlobal { unrelated: boolean }",
        );
        let before_held = graph.stats_snapshot();
        assert_eq!(resolve().id(), cold.id());
        let held = graph.stats_snapshot();
        assert_eq!(
            held.misses, before_held.misses,
            "an open clean batch builds nothing"
        );
        assert_eq!(held.hits - before_held.hits, warm.hits - before.hits);
    });
}

#[test]
fn completed_upsert_is_visible_to_a_cold_query_while_another_batch_is_open() {
    use std::sync::mpsc;
    use verter_type_engine::semantic_query::{ProjectionMode, SemanticNodeData};

    let host = &VerterHost::new_standalone(HostConfig::default());
    let upsert = |canonical: &str, source: &str| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_owned()),
                input_id: canonical.to_owned(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert");
    };
    upsert("/use.ts", "export type Result = NewGlobal;");

    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let held_batch = scope.spawn(move || {
            crate::host_upsert::UPSERT_BATCH_POST_COMMIT_HOOK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    ready_tx.send(()).expect("batch reached post-commit seam");
                    // Dropping the sender also releases the batch on assertion failure.
                    let _ = release_rx.recv();
                }));
            });
            upsert("/held.d.ts", "interface HeldGlobal { held: string }");
        });
        ready_rx.recv().expect("batch is held before publication");

        upsert("/new.d.ts", "interface NewGlobal { value: number }");
        // The consumer has never been queried, and the other batch is still open.
        let answer = host.resolve_named_symbol("/use.ts", "Result", Some(ProjectionMode::Expanded));

        drop(release_tx);
        held_batch.join().expect("held upsert completes");
        let answer = answer.expect("completed contributor resolves on the first cold query");
        let data = host
            .project_type_store()
            .semantic_graph()
            .node_data(answer.id())
            .expect("resolved node");
        let SemanticNodeData::Object(view) = data.as_ref() else {
            panic!("expected the completed global's object, got {data:?}");
        };
        let names: Vec<_> = view
            .positive_members()
            .iter()
            .map(|member| member.string_name().expect("named member").to_string())
            .collect();
        assert_eq!(names, ["value"]);
    });
}

#[test]
fn contributor_ingestion_copies_each_entry_once_per_pass() {
    let results = [4, 8, 12].map(|count| {
        let host = VerterHost::new_standalone(HostConfig::default());
        for ordinal in (0..count).rev() {
            let canonical = format!("/contributor-{ordinal:02}.ts");
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source: Arc::from(format!("interface Registry {{ member{ordinal}: number }}")),
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("upsert contributor");
        }
        let index = host
            .project_type_store()
            .indexed()
            .global_contributor_index();
        index.reset_publish_sorted_entry_count();
        let before = index.snapshot().revision;
        host.ingest_program_ambient_roots();
        let population = index.snapshot();
        let entries = population.lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Registry",
            None,
            true,
        );
        assert_eq!(entries.entries.len(), count);
        assert!(entries
            .entries
            .windows(2)
            .all(|pair| { pair[0].artifact_key.canonical < pair[1].artifact_key.canonical }));
        (
            index.publish_sorted_entry_count(),
            population.revision - before,
        )
    });
    let work = results.map(|(copies, _)| copies);
    assert_eq!(work, [4, 8, 12], "copy each contributor once");
    assert_eq!(work[1] - work[0], 4);
    assert_eq!(work[2] - work[1], 4);
    assert_eq!(results.map(|(_, publications)| publications), [1, 1, 1]);
}

#[test]
fn unpublished_global_keys_reject_validation_until_the_batch_publishes() {
    use crate::project_type_store::IndexedReady;
    use crate::resolver_core::ShallowFileState;
    use verter_session_query::facts::fact_cache::{FactVersionRef, RouteSurfaceFactRef};
    use verter_session_query::facts::registry::FactLane;
    use verter_session_query::facts::store_view::StoreView;
    use verter_session_query::inputs::contributors::population_contributor_fact_key;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/clean.d.ts",
        "interface CleanGlobal { clean: number }",
    );
    upsert(
        &host,
        "/dirty.d.ts",
        "interface DirtyGlobal { before: number }",
    );
    let store = host.project_type_store().indexed();
    let index = store.global_contributor_index();
    let observation = |name: &str| {
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: Arc::from("/use.ts"),
            key: population_contributor_fact_key(&AugmentationTargetKind::GlobalAugmentation, name),
            lane: FactLane::Semantic,
            expected_hash: index.snapshot().observation_fingerprint(
                &AugmentationTargetKind::GlobalAugmentation,
                name,
                None,
            ),
        })
    };
    let clean = observation("CleanGlobal");
    let dirty = observation("DirtyGlobal");
    let absent = observation("AddedGlobal");
    let namespace = observation("AddedNamespace");
    let validates =
        |fact: &FactVersionRef| host.resolver_store_view().view_for_tests().validates(fact);
    assert!(validates(&clean) && validates(&dirty) && validates(&absent) && validates(&namespace));
    let previous = index.snapshot();
    let batch = store.batch_global_contributors();
    let source: Arc<str> = Arc::from(
        "interface DirtyGlobal { after: string } declare var AddedGlobal: boolean; declare namespace AddedNamespace { interface Member {} }",
    );
    let state = ShallowFileState::service_backed_for_test_at("/dirty.d.ts", &source);
    store.insert(
        Arc::from("/dirty.d.ts"),
        Arc::new(IndexedReady::new_for_test_with_state(
            state.whole_hash,
            state,
            Arc::clone(&source),
            source,
        )),
    );
    assert!(
        Arc::ptr_eq(&previous, &index.snapshot()),
        "dirty population is unpublished"
    );
    assert!(
        !validates(&dirty),
        "a changed name cannot reuse its old fingerprint"
    );
    assert!(
        !validates(&absent),
        "a first contributor cannot reuse a proved-empty fingerprint"
    );
    assert!(
        !validates(&namespace),
        "an unpublished namespace cannot validate an empty population"
    );
    assert!(
        validates(&clean),
        "an unrelated dirty name keeps the clean fingerprint valid"
    );
    assert!(
        Arc::ptr_eq(&previous, &index.snapshot()),
        "validation must leave the dirty population unpublished"
    );
    drop(batch);
    assert!(
        validates(&dirty),
        "publishing the same contributor membership restores its population fingerprint"
    );
    assert!(
        !validates(&absent) && !validates(&namespace),
        "published first contributors move their proved-empty fingerprints"
    );
    assert!(validates(&observation("DirtyGlobal")));
    assert!(validates(&observation("AddedGlobal")));
    assert!(validates(&observation("AddedNamespace")));
    index.note_pending_overlay_ambient("/unknown.ts");
    assert!(
        !validates(&clean),
        "an unclassified contributor may declare any name"
    );
}

#[test]
fn declaration_upsert_batch_copies_each_entry_once() {
    let results = [4, 8, 12].map(|count| {
        let host = VerterHost::new_standalone(HostConfig::default());
        let index = host
            .project_type_store()
            .indexed()
            .global_contributor_index();
        let before = index.snapshot().revision;
        let requests = (0..count)
            .rev()
            .map(|ordinal| {
                let canonical = format!("/contributor-{ordinal:02}.d.ts");
                UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source: Arc::from(format!("interface Registry {{ member{ordinal}: number }}")),
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                }
            })
            .collect();
        let outcomes = host
            .upsert_many_with_priority(requests, verter_scheduler::stage::Priority::Interactive);
        assert_eq!(outcomes.len(), count);
        assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
        let publications = index.snapshot().revision - before;
        assert_eq!(
            index
                .snapshot()
                .lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "Registry",
                    None,
                    true,
                )
                .entries
                .len(),
            count
        );
        (index.publish_sorted_entry_count(), publications)
    });
    let work = results.map(|(copies, _)| copies);
    assert_eq!(work, [4, 8, 12]);
    assert_eq!(work[1] - work[0], 4);
    assert_eq!(work[2] - work[1], 4);
    assert_eq!(results.map(|(_, publications)| publications), [1, 1, 1]);
}

#[test]
fn nested_batches_publish_when_each_guard_completes() {
    use crate::file_artifact_store::FileArtifactStore;
    use crate::project_type_store::IndexedReady;
    use crate::resolver_core::ShallowFileState;

    let store = FileArtifactStore::new();
    let index = store.global_contributor_index();
    let insert = |canonical: &str| {
        let source: Arc<str> = Arc::from("interface Registry { member: number }");
        let state = ShallowFileState::service_backed_for_test_at(canonical, &source);
        store.insert(
            Arc::from(canonical),
            Arc::new(IndexedReady::new_for_test_with_state(
                state.whole_hash,
                state,
                Arc::clone(&source),
                source,
            )),
        );
    };
    insert("/middle.d.ts");
    let previous = index.snapshot();
    let outer = store.batch_global_contributors();
    insert("/last.d.ts");
    {
        let _inner = store.batch_global_contributors();
        insert("/first.d.ts");
        assert!(Arc::ptr_eq(&previous, &index.snapshot()));
        assert!(index.has_pending_overlay_ambient());
    }
    // The inner pass must publish before returning, even with the outer open.
    let inner_published = index.snapshot();
    assert_eq!(inner_published.revision, previous.revision + 1);
    assert!(index.has_pending_overlay_ambient());
    drop(outer);
    let published = index.snapshot();
    // Closing the outer pass without more mutations needs no additional swap.
    assert!(Arc::ptr_eq(&inner_published, &published));
    assert_eq!(published.revision, previous.revision + 1);
    assert!(!index.has_pending_overlay_ambient());
    let lookup = |population: &super::GlobalContributorPopulation| {
        population.lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Registry",
            None,
            true,
        )
    };
    let entries = lookup(&published);
    let names: Vec<_> = entries
        .entries
        .iter()
        .map(|entry| &*entry.artifact_key.canonical)
        .collect();
    assert_eq!(names, ["/first.d.ts", "/last.d.ts", "/middle.d.ts"]);
    assert_eq!(lookup(&previous).entries.len(), 1);
    assert_eq!(published.program_snapshot, store.membership_epoch());
}
