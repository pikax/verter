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
    let discovery = include_str!("../project_semantic_dispatch/signature_discovery.rs");
    let nominal = discovery
        .split("fn runtime_nominal(")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn apparent(").next())
        .expect("runtime_nominal body");
    assert!(
        !external.contains("known_canonicals()"),
        "resolve_external_module_augmentation must not scan known_canonicals"
    );
    assert!(
        !nominal.contains("known_canonicals()"),
        "the lib runtime nominal's contributors must not scan known_canonicals"
    );
    assert!(
        nominal.contains("current_request_canonical"),
        "the lib runtime nominal must use the request's compiler options"
    );
    assert!(
        !nominal.contains("snapshot_canonicals()"),
        "the lib runtime nominal must not pick the first workspace file"
    );
    assert!(
        !nominal.contains("overlay_canonicals()"),
        "the lib runtime nominal must not pick the first overlay file"
    );
    let collect = build
        .split("fn collect_augmentation_contributions(")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn ").next())
        .expect("collect_augmentation_contributions body");
    assert!(
        collect.contains("enum OrderedOrigin"),
        "augmenter and file-scope contributors must be ordered together before lowering"
    );
    assert!(
        !collect.contains("let mut augmenter_order"),
        "augmenter-only pre-sort must not run before file-scope lowering"
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

#[test]
fn upserted_unimported_ambient_is_ingested_on_standalone_host() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/aug.d.ts".to_owned()),
            input_id: "/aug.d.ts".to_owned(),
            source: Arc::from(
                "declare module \"ext-pkg\" { export interface Cfg { mode: string } }\n",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("augmenter upsert");
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/aug.d.ts")
            .is_some(),
        "upserted ambient declare-module .d.ts must be IndexedReady without a later demand"
    );
    let hit = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot()
        .lookup_in_space(
            &AugmentationTargetKind::ExternalSpecifier(
                crate::file_artifact_store::InternedSpecifier::from("ext-pkg"),
            ),
            "Cfg",
            None,
            true,
            verter_semantic::facts::SymbolSpace::Type,
        );
    assert!(
        !hit.entries.is_empty(),
        "upserted overlay-only ambient .d.ts must enter the population"
    );
}

#[test]
fn export_only_snapshot_dts_is_not_eagerly_indexed() {
    let workspace = std::sync::Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/pkg/index.d.ts".to_owned(),
        Arc::from("export interface Props { msg: string }\n"),
    );
    workspace.inject_file("/app.ts".to_owned(), Arc::from("export const n = 1;\n"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/pkg/index.d.ts")
            .is_none(),
        "export-only snapshot .d.ts must stay cold until demanded"
    );
}

#[test]
fn unrelated_file_publish_sorts_no_contributor_entries() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/promise.d.ts",
        "export {};\ndeclare global { interface Promise<T> { thenable: T } }\n",
    );
    let index = store.global_contributor_index();
    index.reset_publish_sorted_entry_count();
    publish(&store, "/unrelated.ts", "export const x = 1;\n");
    assert_eq!(
        index.publish_sorted_entry_count(),
        0,
        "an unrelated file must not clone/sort existing contributor vectors"
    );
}

#[test]
fn source_has_ambient_contribution_matches_declare_module_and_global() {
    use super::source_has_ambient_contribution;
    assert!(source_has_ambient_contribution(
        "declare module \"ext-pkg\" { export const x: number }\n"
    ));
    assert!(source_has_ambient_contribution(
        "export {};\ndeclare global { interface Window { x: number } }\n"
    ));
    assert!(source_has_ambient_contribution(
        "export declare module \"vite/client\" { interface ImportMeta { env: unknown } }\n"
    ));
    assert!(!source_has_ambient_contribution(
        "export interface Props { msg: string }\n"
    ));
    assert!(!source_has_ambient_contribution(
        "import type { P } from './dep';\nexport type Owner = P;\n"
    ));
    assert!(!source_has_ambient_contribution(
        "// declare module \"nope\" { }\nexport const x = 1;\n"
    ));
}

#[test]
fn ordinary_script_file_scope_globals_index_before_population_lookup() {
    use super::source_may_have_file_scope_global_contribution;
    assert!(source_may_have_file_scope_global_contribution(
        "interface Window { x: number }\n"
    ));
    assert!(source_may_have_file_scope_global_contribution(
        "namespace N { export const v = 1; }\n"
    ));
    assert!(!source_may_have_file_scope_global_contribution(
        "export {};\ninterface Window { x: number }\n"
    ));
    assert!(!source_may_have_file_scope_global_contribution(
        "export const n = 1;\n"
    ));
    for source in [
        "if ((ok)) /; export {}/.test(x); interface Window {}",
        "while (ok) /; export {}/.test(x); interface Window {}",
        "for (; ok;) /; export {}/.test(x); interface Window {}",
        "obj.if(ok) / n; interface Window {}",
    ] {
        assert!(
            source_may_have_file_scope_global_contribution(source),
            "{source}"
        );
    }
    assert!(!source_may_have_file_scope_global_contribution(
        "obj.if(ok) / n; export {}; interface Window {}"
    ));

    let workspace = std::sync::Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/globals.ts".to_owned(),
        Arc::from("if (ok) /; export {}/.test(x); interface Window { x: number }\n"),
    );
    workspace.inject_file("/plain.ts".to_owned(), Arc::from("export const n = 1;\n"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/globals.ts")
            .is_none(),
        "ordinary script globals must stay cold until population lookup"
    );
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/plain.ts")
            .is_none(),
        "unrelated ordinary scripts must not be indexed"
    );
    host.ingest_program_ambient_roots();
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/globals.ts")
            .is_some(),
        "file-scope script globals must be IndexedReady before population lookup"
    );
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/plain.ts")
            .is_none(),
        "unrelated ordinary scripts must stay cold after population lookup"
    );
    let hit = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot()
        .lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "Window",
            None,
            true,
        );
    assert!(
        !hit.entries.is_empty(),
        "never-upserted ordinary script interface must enter the population"
    );
}

#[test]
fn upsert_does_not_rescan_snapshot_when_membership_is_unchanged() {
    let workspace = std::sync::Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/ambient-test.d.ts".to_owned(),
        Arc::from("declare module \"ambient-test\" { export const x: number }\n"),
    );
    workspace.inject_file("/app.ts".to_owned(), Arc::from("export const n = 1;\n"));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let index = host
        .project_type_store()
        .indexed()
        .global_contributor_index();
    index.reset_snapshot_scan_visit_count();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/app.ts".to_owned()),
            input_id: "/app.ts".to_owned(),
            source: Arc::from("export const n = 2;\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    assert_eq!(
        index.snapshot_scan_visit_count(),
        0,
        "an ordinary edit must not re-walk snapshot members"
    );
    host.ingest_program_ambient_roots();
    host.ingest_program_ambient_roots();
    assert_eq!(
        index.snapshot_scan_visit_count(),
        0,
        "warm population lookups must also skip enumeration"
    );
}

#[test]
fn snapshot_replacement_discovers_new_globals_without_membership_change() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file("/changed.ts".into(), Arc::from("export const x = 1;"));
    workspace.inject_file(
        "/module.ts".into(),
        Arc::from("export {}; interface LocalOnly { x: 1 }"),
    );
    let host = VerterHost::new(HostConfig::default(), workspace.clone());
    host.ingest_program_ambient_roots();
    workspace.inject_file(
        "/changed.ts".into(),
        Arc::from("interface AddedGlobal { x: 1 }"),
    );
    host.ingest_program_ambient_roots();
    let population = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot();
    assert_eq!(
        population
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                "AddedGlobal",
                None,
                true
            )
            .entries
            .len(),
        1
    );
    assert!(population
        .lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            "LocalOnly",
            None,
            true
        )
        .entries
        .is_empty());
}
