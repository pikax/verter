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
    let build = include_str!("../../../verter_type_engine/src/project_semantic_dispatch/build.rs");
    let external = build
        .split("fn resolve_external_module_augmentation(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub(super) fn ").next())
        .expect("resolve_external_module_augmentation body");
    let discovery = include_str!(
        "../../../verter_type_engine/src/project_semantic_dispatch/signature_discovery.rs"
    );
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
        .split("fn fold_augmentation_contributions(")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn ").next())
        .expect("fold_augmentation_contributions body");
    assert!(
        collect.contains("augmentation_contributor_order"),
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
        Some(verter_type_engine::semantic_query::ProjectionMode::Expanded),
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
            verter_session_query::facts::SymbolSpace::Value,
        );
    assert!(
        !hit.entries.is_empty(),
        "an injected unimported ambient .d.ts must enter the population without being upserted"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn contributor_ingestion_takes_admission_before_its_cursor() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    workspace.inject_file(
        "/cursor.d.ts".into(),
        Arc::from("declare const cursor: number;"),
    );
    let unfenced = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let observed = unfenced.clone();
    let weak = Arc::downgrade(&host);
    *host.test_force.ambient_ingestion_cursor_hook.0.lock() = Some(Arc::new(move || {
        let host = weak.upgrade().unwrap();
        let admission_available = std::thread::spawn(move || {
            let admission = host.block_content.admission_fence.try_lock();
            admission.is_some()
        })
        .join()
        .unwrap();
        observed.store(admission_available, std::sync::atomic::Ordering::SeqCst);
    }));
    host.ingest_injected_ambient_roots(None);
    assert!(
        !unfenced.load(std::sync::atomic::Ordering::SeqCst),
        "a cursor holder must never wait behind ingress admission"
    );
    assert!(host
        .project_type_store()
        .indexed()
        .get_any("/cursor.d.ts")
        .is_some());
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
            verter_session_query::facts::SymbolSpace::Type,
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
    // A byte-order mark opens no token.
    assert!(source_has_ambient_contribution(
        "\u{FEFF}declare module \"m\" { const v: 1; }\n"
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
    // A script's file-scope value declarations are globals too.
    for source in [
        "var sv = 1;\n",
        "let sl = 1;\n",
        "const sc = 1;\n",
        "function sf() {}\n",
        "async function saf() {}\n",
        "declare var dv: number;\n",
        "declare function df(): void;\n",
        "declare const dc: string;\n",
        // A `var` in a nested block hoists to the top level.
        "if (ok) { var nested = 1; }\n",
        "for (var i = 0; i < 1; i++) {}\n",
        // Every opening of a file-scope declaration keeps a script in: a
        // decorator, a byte-order mark, a comment before `declare`, a type
        // alias and the legacy `module` keyword.
        "@dec class C { x = 1 }\n",
        "@dec()\nabstract class A {}\n",
        "\u{FEFF}class Bom {}\n",
        "/* c */ declare class Dc {}\n",
        "type TT = { q: 1 };\n",
        "declare type DT = { r: 2 };\n",
        "module Mo { export type MT = 3; }\n",
    ] {
        assert!(
            source_may_have_file_scope_global_contribution(source),
            "{source}"
        );
    }
    for source in [
        "if (ok) { const inner = 1; }\n",
        "constant(1);\n",
        "x.var = 1;\n",
        "export {};\nvar sv = 1;\n",
        "declare module \"m\" { const v: 1; }\n",
    ] {
        assert!(
            !source_may_have_file_scope_global_contribution(source),
            "{source}"
        );
    }
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
fn script_file_scope_values_enter_the_population_by_name() {
    use verter_session_query::facts::SymbolSpace;
    use verter_session_query::inputs::contributors::ContributorOrigin;
    let workspace = std::sync::Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/script.ts".to_owned(),
        Arc::from("var scriptVar = 1;\nfunction scriptFn(): string { return \"\"; }\n"),
    );
    workspace.inject_file(
        "/module.ts".to_owned(),
        Arc::from("export {};\nvar moduleVar = 1;\n"),
    );
    let host = VerterHost::new(HostConfig::default(), workspace);
    host.ingest_program_ambient_roots();
    let snapshot = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot();
    let values = |name: &str| {
        snapshot.lookup_in_space(
            &AugmentationTargetKind::GlobalAugmentation,
            name,
            None,
            true,
            SymbolSpace::Value,
        )
    };
    for name in ["scriptVar", "scriptFn"] {
        let hit = values(name);
        assert_eq!(hit.entries.len(), 1, "{name}");
        assert_eq!(hit.entries[0].origin, ContributorOrigin::FileScopeValue);
    }
    assert!(
        values("moduleVar").entries.is_empty(),
        "a module's top-level value is not a global"
    );
    // One observation covers the value space too: a value contributor moves
    // the fingerprint a reader of the name recorded.
    let empty = snapshot.lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "scriptVar",
        None,
        true,
    );
    assert!(empty.entries.is_empty(), "no type-space contributor");
    assert_ne!(
        snapshot.observation_fingerprint(
            &AugmentationTargetKind::GlobalAugmentation,
            "scriptVar",
            None
        ),
        empty.fingerprint,
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

/// A script's class, type alias and enum enter the type-space population as
/// file-scope type contributors, and leave it with their artifact: an edit
/// that supersedes the script's version drops the old version's records,
/// and removing the script drops every record it held.
#[test]
fn script_file_scope_types_are_released_on_supersession_and_removal() {
    use verter_session_query::inputs::contributors::ContributorOrigin;
    let host = VerterHost::new(
        HostConfig::default(),
        Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        )),
    );
    let upsert = |source: &str| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some("/script.ts".to_owned()),
                input_id: "/script.ts".to_owned(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert");
        host.ingest_program_ambient_roots();
    };
    let index = host
        .project_type_store()
        .indexed()
        .global_contributor_index();
    let types = |name: &str| {
        index
            .snapshot()
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                name,
                None,
                true,
            )
            .entries
            .iter()
            .map(|entry| entry.origin)
            .collect::<Vec<_>>()
    };
    upsert("class Pc { x = 1 }\ntype TT = { q: 1 };\nenum GE { A }\n");
    for name in ["Pc", "TT", "GE"] {
        assert_eq!(types(name), [ContributorOrigin::FileScopeType], "{name}");
    }
    upsert("class Other {}\n");
    for name in ["Pc", "TT", "GE"] {
        assert!(types(name).is_empty(), "a superseded {name} is released");
    }
    assert_eq!(types("Other"), [ContributorOrigin::FileScopeType]);
    host.remove("/script.ts");
    assert!(
        types("Other").is_empty(),
        "a removed script's type is released"
    );
    assert!(
        !index.record_canonicals().contains("/script.ts"),
        "a removed script keeps no contribution record"
    );
}

/// Every declaring base of a merged global demands its contributors: the
/// space-filtered answer, the type+namespace answer and the observation
/// fingerprint. One population serves them all, so the entries those
/// demands copy grow with the contributors added, never with
/// bases × contributors.
#[test]
fn all_base_demands_share_one_population_and_copy_proportional_to_additions() {
    use verter_session_query::facts::SymbolSpace;
    let target = AugmentationTargetKind::GlobalAugmentation;
    let mut copied_per_size: Vec<(usize, u64)> = Vec::new();
    for size in [128usize, 256, 512, 1024] {
        let store = FileArtifactStore::new();
        for index in 0..size {
            let canonical = format!("/merged/decl{index:04}.d.ts");
            // Script file-scope interfaces and namespaces and module
            // `declare global` blocks all declare the one merged global.
            let source = if index % 4 == 0 {
                format!(
                    "interface Merged {{ m{index}: number }}\nnamespace Merged {{ export const n{index} = 1; }}\n"
                )
            } else if index % 2 == 0 {
                format!("interface Merged {{ m{index}: number }}\n")
            } else {
                format!(
                    "export {{}};\ndeclare global {{ interface Merged {{ m{index}: number }} }}\n"
                )
            };
            publish(&store, &canonical, &source);
        }
        let snapshot = store.global_contributor_index().snapshot();
        let first_in_space =
            snapshot.lookup_in_space(&target, "Merged", None, true, SymbolSpace::Type);
        let first_merged = snapshot.lookup(&target, "Merged", None, true);
        let observed = snapshot.observation_fingerprint(&target, "Merged", None);
        assert_eq!(
            first_in_space.entries.len(),
            size,
            "one entry per declaration"
        );
        assert_eq!(
            first_merged.entries.len(),
            size + size / 4,
            "a file's interface and namespace are both contributors"
        );
        assert_eq!(observed, first_merged.fingerprint);
        let canonicals: Vec<&str> = first_merged
            .entries
            .iter()
            .map(|entry| entry.artifact_key.canonical.as_ref())
            .collect();
        let mut sorted = canonicals.clone();
        sorted.sort_unstable();
        assert_eq!(
            canonicals, sorted,
            "contributor order is the population order"
        );

        for _base in 0..size {
            let in_space =
                snapshot.lookup_in_space(&target, "Merged", None, true, SymbolSpace::Type);
            let merged = snapshot.lookup(&target, "Merged", None, true);
            assert!(Arc::ptr_eq(&in_space.entries, &first_in_space.entries));
            assert!(Arc::ptr_eq(&merged.entries, &first_merged.entries));
            assert_eq!(
                snapshot.observation_fingerprint(&target, "Merged", None),
                observed
            );
        }
        copied_per_size.push((size, snapshot.materialized_view_entry_count()));

        // A later declaration publishes a new population; the earlier
        // snapshot's answers stay what they were.
        publish(
            &store,
            "/merged/later.d.ts",
            "interface Merged { later: number }\n",
        );
        let next = store.global_contributor_index().snapshot();
        let grown = next.lookup_in_space(&target, "Merged", None, true, SymbolSpace::Type);
        assert_eq!(grown.entries.len(), size + 1);
        assert_ne!(grown.fingerprint, first_in_space.fingerprint);
        assert_eq!(first_in_space.entries.len(), size);
    }
    for &(size, copied) in &copied_per_size {
        assert!(
            copied <= 3 * size as u64,
            "{size} bases copied {copied} entries: one copy per view, not per base"
        );
    }
    let (smallest, smallest_copied) = copied_per_size[0];
    let (largest, largest_copied) = copied_per_size[copied_per_size.len() - 1];
    assert_eq!(
        largest_copied * smallest as u64,
        smallest_copied * largest as u64,
        "copies grow linearly with the contributors: {copied_per_size:?}"
    );
}

#[test]
fn project_filtered_population_is_shared_and_tracks_membership_revision() {
    use std::cell::Cell;
    use verter_session_query::facts::SymbolSpace;
    use verter_session_query::resolution::ProjectId;
    let store = FileArtifactStore::new();
    for i in 0..128 {
        publish(
            &store,
            &format!("/p{}/decl{i:04}.d.ts", i % 2),
            &format!("interface Shared {{ p{i}: {i} }}"),
        );
    }
    let snapshot = store.global_contributor_index().snapshot();
    let visits = Cell::new(0);
    let lookup = |project, generation, prefix: &str| {
        snapshot
            .lookup_in_project(
                "Shared",
                SymbolSpace::Type,
                None,
                Some(ProjectId(project)),
                Some(generation),
                |entry| {
                    visits.set(visits.get() + 1);
                    entry.artifact_key.canonical.starts_with(prefix)
                },
                |_| {
                    Some(Arc::new(
                        verter_session_query::inputs::program_order::ProgramFileOrder::unordered(),
                    ))
                },
            )
            .0
    };
    let first = lookup(0, 1, "/p0/");
    assert_eq!(first.entries.len(), 64);
    let visits_after_first = visits.get();
    for _ in 0..128 {
        let again = lookup(0, 1, "/p0/");
        assert!(Arc::ptr_eq(&first.entries, &again.entries));
    }
    assert_eq!(
        visits.get(),
        visits_after_first,
        "repeated demands do not scan or copy contributors"
    );
    let other = lookup(1, 1, "/p1/");
    assert!(other
        .entries
        .iter()
        .all(|entry| entry.artifact_key.canonical.starts_with("/p1/")));
    let moved = lookup(0, 2, "/p1/");
    assert_eq!(
        moved
            .entries
            .iter()
            .map(|entry| entry.artifact_key.clone())
            .collect::<Vec<_>>(),
        other
            .entries
            .iter()
            .map(|entry| entry.artifact_key.clone())
            .collect::<Vec<_>>(),
        "ownership change replaces the filtered view"
    );
    assert_eq!(
        first.fingerprint, moved.fingerprint,
        "population fact stays on the unfiltered observation rail"
    );
    assert!(first
        .entries
        .iter()
        .all(|entry| entry.artifact_key.canonical.starts_with("/p0/")));
}

#[test]
fn every_base_borrows_one_augmentation_collection_order() {
    use verter_session_query::inputs::contributors::AugmentationCandidate;
    use verter_session_query::inputs::program_order::ProgramFileOrder;
    use verter_session_query::resolution::{
        AugmentationPopulation, AugmentationTargetKey, AugmenterSet, ProjectIdentity,
    };
    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([0; 16]),
        resolve_env_hash: [0; 16],
        lib_env_hash: [0; 16],
        population: AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let augmenters = AugmenterSet {
        entries: Default::default(),
        fingerprint: [0; 16],
    };
    for size in [128usize, 512, 2048] {
        let store = FileArtifactStore::new();
        let names: Vec<String> = (0..size).map(|i| format!("/decl{i:04}.d.ts")).collect();
        for (i, name) in names.iter().enumerate() {
            publish(&store, name, &format!("interface Shared {{ p{i}: {i} }}"));
        }
        let program = |roots: Vec<&String>| {
            Arc::new(ProgramFileOrder::sequence(
                Arc::from("/tsconfig.json"),
                roots.into_iter().map(|name| Arc::from(name.as_str())),
                |_| Some(Vec::new()),
            ))
        };
        let listed = program(names.iter().collect());
        let first_last = program(names[1..].iter().chain(&names[..1]).collect());
        let snapshot = store.global_contributor_index().snapshot();
        let contributors = snapshot.lookup(&key.target, "Shared", None, true);
        let lookup = |order: &Arc<ProgramFileOrder>| {
            let (view, observed) = snapshot.augmentation_order(
                &key,
                &augmenters,
                "Shared",
                &contributors,
                verter_session_query::inputs::contributors::FileScopeContribution::TypeBody,
                |_| Some(Arc::clone(order)),
            );
            assert!(observed.is_some_and(|observed| Arc::ptr_eq(&observed, order)));
            view
        };
        let first = lookup(&listed);
        assert_eq!(first.len(), size);
        let copied = snapshot.materialized_view_entry_count();
        let work = snapshot.ordering_work_count();
        for _ in 0..size {
            assert!(Arc::ptr_eq(&first, &lookup(&listed)));
        }
        assert_eq!(
            snapshot.ordering_work_count(),
            work,
            "base demands do not reorder the population"
        );
        assert_eq!(
            snapshot.materialized_view_entry_count(),
            copied,
            "base demands copy no contributor slots"
        );
        assert!(matches!(first[0], AugmentationCandidate::FileScope(0)));
        let reordered = lookup(&first_last);
        assert!(
            matches!(reordered[size - 1], AugmentationCandidate::FileScope(0)),
            "the program order decides precedence"
        );
    }
}

/// Inject and upsert `size` files, one in twenty contributing to a global
/// interface, then ingest the program's ambient roots: the snapshot visits
/// stay within two per file and the interface has every contributor. Returns
/// the snapshot visits.
fn interleaved_inject_and_upsert_visits(size: u64) -> u64 {
    {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), workspace.clone());
        let index = host
            .project_type_store()
            .indexed()
            .global_contributor_index();
        index.reset_snapshot_scan_visit_count();
        for n in 0..size {
            let canonical = format!("/open/file{n}.ts");
            let source: Arc<str> = if n % 20 == 0 {
                Arc::from(format!(
                    "export {{}}; declare global {{ interface Shared {{ m{n}: number }} }}"
                ))
            } else {
                Arc::from(format!("export const v{n} = {n};"))
            };
            workspace.inject_file(canonical.clone(), source.clone());
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source,
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("upsert");
        }
        host.ingest_program_ambient_roots();
        assert!(
            index.snapshot_scan_visit_count() <= 2 * size,
            "{size} files required {} snapshot visits",
            index.snapshot_scan_visit_count()
        );
        assert_eq!(
            index
                .snapshot()
                .lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "Shared",
                    None,
                    true,
                )
                .entries
                .len(),
            (size as usize).div_ceil(20)
        );
        index.snapshot_scan_visit_count()
    }
}

/// The same files at 50, 100 and 150: the gate version of the 250-, 1,000-
/// and 4,000-file test in the nightly scale tier. Each size classifies every
/// contributor within two snapshot visits per file; the visits grow by the
/// same amount per step, so they are affine in the files, and at 250, 1,000
/// and 4,000 files they stay within two per file.
#[test]
fn interleaved_inject_and_upsert_visits_are_affine_and_fit_four_thousand_files() {
    let sizes = [50, 100, 150];
    let work = crate::scale_gate::affine(
        "interleaved inject and upsert",
        sizes,
        vec![crate::scale_gate::Counter {
            name: "snapshot visits",
            values: sizes
                .iter()
                .map(|&size| interleaved_inject_and_upsert_visits(size as u64))
                .collect(),
        }],
    );
    for size in [250, 1_000, 4_000] {
        let visits = work.at("snapshot visits", size);
        assert!(
            visits <= 2 * size as u64,
            "{size} files would require {visits} snapshot visits"
        );
    }
}

#[test]
fn configured_project_opening_classifies_linear_members() {
    let mut previous_visits = None;
    // Every size spans all directory buckets. Count classification work across
    // four doublings instead of repeating a large project's scheduler waits.
    for size in [125, 250, 500, 1_000] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), workspace.clone());
        workspace.inject_file(
            "/open/tsconfig.json".into(),
            Arc::from(r#"{"compilerOptions":{"noLib":true},"include":["**/*.ts"]}"#),
        );
        let mut project = verter_workspace::ide_project_config(
            "/open".into(),
            "/open".into(),
            Some("/open/tsconfig.json".into()),
        );
        project.compiler_options =
            verter_workspace::load_compiler_options(workspace.as_ref(), "/open/tsconfig.json");
        host.configure_projects(vec![project]);
        workspace.inject_file(
            "/open/lib.d.ts".into(),
            Arc::from("interface SharedRoot { value: number }"),
        );
        let index = host
            .project_type_store()
            .indexed()
            .global_contributor_index();
        index.reset_snapshot_scan_visit_count();
        for n in 0..size {
            let canonical = format!("/open/dir{}/file{n}.ts", n % 104);
            let source: Arc<str> = if n % 20 == 0 {
                Arc::from(format!(
                    "export {{}}; declare global {{ interface Shared {{ m{n}: number }} }}"
                ))
            } else {
                Arc::from(format!(
                    "export const v{n} = {n}; export type Root{n} = SharedRoot;"
                ))
            };
            workspace.inject_file(canonical.clone(), source.clone());
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source,
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("upsert");
        }
        host.ingest_program_ambient_roots();
        let visits = index.snapshot_scan_visit_count();
        eprintln!("opened {size} configured files: {visits} snapshot visits",);
        assert!(
            visits <= 2 * size,
            "{size} files required {visits} snapshot visits",
        );
        if let Some(previous) = previous_visits {
            assert!(
                visits <= 2 * previous,
                "doubling configured members grew snapshot visits from {previous} to {visits}",
            );
        }
        previous_visits = Some(visits);
        assert_eq!(
            index
                .snapshot()
                .lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "Shared",
                    None,
                    true,
                )
                .entries
                .len(),
            (size as usize).div_ceil(20)
        );
        assert_eq!(
            index
                .snapshot()
                .lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "SharedRoot",
                    None,
                    true,
                )
                .entries
                .len(),
            1,
            "the never-upserted shared library remains a global contributor"
        );
    }
}

#[test]
fn never_upserted_contributors_follow_snapshot_edits_and_removals() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), workspace.clone());
    workspace.inject_file(
        "/global.ts".into(),
        Arc::from("export {}; declare global { interface X { global: number } }"),
    );
    workspace.inject_file(
        "/script.ts".into(),
        Arc::from("interface X { script: string }"),
    );
    for n in 0..250 {
        let canonical = format!("/bulk{n}.ts");
        let source: Arc<str> = Arc::from("export const n = 1;");
        workspace.inject_file(canonical.clone(), source.clone());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical,
                source,
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert");
    }
    let index = host
        .project_type_store()
        .indexed()
        .global_contributor_index();
    let contributors = |name: &str| {
        host.ingest_program_ambient_roots();
        index.snapshot().lookup(
            &AugmentationTargetKind::GlobalAugmentation,
            name,
            None,
            true,
        )
    };
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/use.ts".into()),
            input_id: "/use.ts".into(),
            source: Arc::from("export type Result = X;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("consumer upsert");
    let answer = || {
        use verter_type_engine::semantic_query::{ProjectionMode, SemanticNodeData};
        let node = host
            .resolve_named_symbol("/use.ts", "Result", Some(ProjectionMode::Expanded))
            .expect("global type resolves");
        let store = host.project_type_store().semantic_graph();
        let mut pending = vec![node.id()];
        let mut names = Vec::new();
        while let Some(node) = pending.pop() {
            let data = store.node_data(node).expect("node");
            match data.as_ref() {
                SemanticNodeData::Object(view) => names.extend(
                    view.positive_members()
                        .iter()
                        .map(|member| member.string_name().expect("named property").to_string()),
                ),
                SemanticNodeData::MergedDecl { contributors } => {
                    pending.extend(contributors.iter().copied())
                }
                other => panic!("expected X contributor object: {other:?}"),
            }
        }
        names.sort();
        names
    };
    let original = contributors("X");
    assert_eq!(answer(), ["global", "script"]);
    assert_eq!(original.entries.len(), 2);
    workspace.inject_file(
        "/global.ts".into(),
        Arc::from("export {}; declare global { interface X { edited: boolean } }"),
    );
    let edited = contributors("X");
    assert_eq!(edited.entries.len(), 2);
    assert_eq!(
        edited.fingerprint, original.fingerprint,
        "member content changes preserve the same contributor population"
    );
    assert_eq!(
        answer(),
        ["edited", "script"],
        "actual member content remains independently invalidating"
    );
    workspace.inject_file(
        "/script.ts".into(),
        Arc::from("export {}; interface X { private: number }"),
    );
    assert_eq!(contributors("X").entries.len(), 1);
    assert_eq!(answer(), ["edited"]);
    workspace.remove_file("/global.ts");
    assert!(contributors("X").entries.is_empty());
    workspace.inject_file(
        "/pending.ts".into(),
        Arc::from("interface Pending { x: number }"),
    );
    host.ingest_injected_ambient_roots(None);
    workspace.remove_file("/pending.ts");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/use.ts".into()),
            input_id: "/use.ts".into(),
            source: Arc::from("export type Result = X; export const changed = true;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert drains a pending removal while holding admission");
    assert!(contributors("Pending").entries.is_empty());
    assert!(!index.has_pending_overlay_ambient());
}

#[test]
fn snapshot_changes_preserve_overlay_contributors_and_independent_hosts() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let first = VerterHost::new(HostConfig::default(), workspace.clone());
    let second = VerterHost::new(HostConfig::default(), workspace.clone());
    workspace.inject_file(
        "/shared.d.ts".into(),
        Arc::from("interface SharedRoot { disk: number }"),
    );
    for host in [&first, &second] {
        host.ingest_program_ambient_roots();
        assert_eq!(
            host.project_type_store()
                .indexed()
                .global_contributor_index()
                .snapshot()
                .lookup(
                    &AugmentationTargetKind::GlobalAugmentation,
                    "SharedRoot",
                    None,
                    true
                )
                .entries
                .len(),
            1
        );
    }
    let _ = first
        .upsert(UpsertRequest {
            canonical_id: Some("/shared.d.ts".into()),
            input_id: "/shared.d.ts".into(),
            source: Arc::from("interface OverlayRoot { editor: string }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("overlay upsert");
    first.ingest_program_ambient_roots();
    workspace.inject_file(
        "/shared.d.ts".into(),
        Arc::from("interface ChangedDisk { n: number }"),
    );
    workspace.remove_file("/shared.d.ts");
    first.ingest_program_ambient_roots();
    let population = first
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot();
    assert_eq!(
        population
            .lookup(
                &AugmentationTargetKind::GlobalAugmentation,
                "OverlayRoot",
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
            "ChangedDisk",
            None,
            true
        )
        .entries
        .is_empty());
}

/// Full-size workloads, run by the nightly scale tier (`.config/nextest.toml`).
/// Each has a gate version outside this module.
mod nightly_scale {
    use super::*;

    #[test]
    fn interleaved_inject_and_upsert_classifies_linear_members() {
        for size in [250, 1_000, 4_000] {
            interleaved_inject_and_upsert_visits(size);
        }
    }
}
