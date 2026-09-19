//! Lookup of a global symbol performs no whole-program membership scan.

use std::sync::Arc;

use crate::file_artifact_store::{AugmentationTargetKind, FileArtifactStore};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::ShallowFileState;
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

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

#[test]
fn lookup_path_source_has_no_known_canonicals_scan() {
    let build = include_str!("../project_semantic_dispatch/build.rs");
    let external = build
        .split("fn resolve_external_module_augmentation(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub(super) fn ").next())
        .expect("resolve_external_module_augmentation body");
    let nominal = build
        .split("fn runtime_nominal_call_signatures(")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn first_parameter(").next())
        .expect("runtime_nominal_call_signatures body");
    assert!(
        !external.contains("known_canonicals()"),
        "resolve_external_module_augmentation must not scan known_canonicals"
    );
    assert!(
        !nominal.contains("known_canonicals()"),
        "runtime_nominal_call_signatures must not scan known_canonicals"
    );
}

#[test]
fn unrelated_file_does_not_move_an_augmented_symbol_fingerprint() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/promise.d.ts",
        "export {};\ndeclare global { interface Promise<T> { thenable: T } }\n",
    );
    let before = store
        .global_contributor_index()
        .snapshot()
        .lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Promise",
            None,
            true,
        )
        .fingerprint;
    publish(&store, "/unrelated.ts", "export const x = 1;\n");
    let after = store
        .global_contributor_index()
        .snapshot()
        .lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Promise",
            None,
            true,
        )
        .fingerprint;
    assert_eq!(
        before, after,
        "an unrelated file must not invalidate an augmented primitive's contributor fingerprint"
    );
}

#[test]
fn lookup_does_not_call_known_canonicals() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
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
    upsert(
        "/promise.d.ts",
        "export {};\ndeclare global { interface Promise<T> { (value: T): void } }\n",
    );
    upsert(
        "/use.ts",
        "declare const la: Awaited<{ then(onfulfilled: Promise<number>): void }>;\n\
         export function fa() { return la; }\n",
    );
    let _ = host.ensure_indexed_ready("/promise.d.ts");
    let _ = host.ensure_indexed_ready("/use.ts");
    verter_workspace::reset_known_canonicals_calls();
    let _ = host.resolve_named_symbol(
        "/use.ts",
        "fa",
        Some(crate::semantic_query::ProjectionMode::Expanded),
    );
    assert_eq!(
        verter_workspace::known_canonicals_calls(),
        0,
        "augmentation lookup must not scan known_canonicals"
    );
}

#[test]
fn injected_unimported_ambient_is_ingested_without_upserting_it() {
    let workspace = std::sync::Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/ambient-test.d.ts".to_owned(),
        Arc::from("declare module \"ambient-test\" { export const x: number }\n"),
    );
    let host = VerterHost::new(HostConfig::default(), workspace);
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/use.ts".to_owned()),
            input_id: "/use.ts".to_owned(),
            source: Arc::from("import { x } from \"ambient-test\"; export const y = x;\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("consumer upsert");
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/ambient-test.d.ts")
            .is_some(),
        "injected ambient .d.ts must be IndexedReady after consumer upsert"
    );
    let hit = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot()
        .lookup_in_space(
            &AugmentationTargetKind::ExternalSpecifier(
                crate::file_artifact_store::InternedSpecifier::from("ambient-test"),
            ),
            "x",
            None,
            true,
            verter_semantic::facts::SymbolSpace::Value,
        );
    assert!(
        !hit.entries.is_empty(),
        "an injected unimported ambient .d.ts must enter the population without being upserted"
    );
}
