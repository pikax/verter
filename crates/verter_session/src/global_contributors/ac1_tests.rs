//! First-contributor transitions for global symbol populations.

use std::sync::Arc;

use super::{classify_module_kind, is_automatic_lib_canonical, ContributorOrigin, FileModuleKind};
use crate::file_artifact_store::{
    AugmentationTargetKind, FileArtifactStore, GLOBAL_AUGMENTATION_TAG,
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

fn lookup_global(store: &FileArtifactStore, name: &str) -> super::SymbolContributors {
    store.global_contributor_index().snapshot().lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        name,
        None,
        true,
    )
}

#[test]
fn first_declare_global_contributor_invalidates_empty_fingerprint() {
    let store = FileArtifactStore::new();
    let empty = lookup_global(&store, "Window");
    assert!(empty.entries.is_empty());
    let empty_fp = empty.fingerprint;

    publish(
        &store,
        "/globals.d.ts",
        "export {};\ndeclare global { interface Window { x: number } }\n",
    );
    let populated = lookup_global(&store, "Window");
    assert_eq!(populated.entries.len(), 1);
    assert_eq!(
        populated.entries[0].origin,
        ContributorOrigin::DeclareGlobal
    );
    assert_ne!(
        populated.fingerprint, empty_fp,
        "adding the first contributor must move the proved-empty fingerprint"
    );

    store.remove("/globals.d.ts");
    let after = lookup_global(&store, "Window");
    assert!(after.entries.is_empty());
    assert_eq!(
        after.fingerprint, empty_fp,
        "removing the last contributor restores the empty fingerprint"
    );
}

#[test]
fn renaming_a_symbol_drops_the_old_name_and_never_keeps_a_stale_positive() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/g.d.ts",
        "export {};\ndeclare global { interface Foo { a: 1 } }\n",
    );
    assert_eq!(lookup_global(&store, "Foo").entries.len(), 1);
    assert!(lookup_global(&store, "Bar").entries.is_empty());

    publish(
        &store,
        "/g.d.ts",
        "export {};\ndeclare global { interface Bar { a: 1 } }\n",
    );
    assert!(
        lookup_global(&store, "Foo").entries.is_empty(),
        "the renamed-away symbol must not stay a stale positive"
    );
    assert_eq!(lookup_global(&store, "Bar").entries.len(), 1);
}

#[test]
fn script_to_module_flip_removes_file_scope_global_interface() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/script.ts",
        "interface Window { fromScript: true }\n",
    );
    let as_script = lookup_global(&store, "Window");
    assert_eq!(as_script.entries.len(), 1);
    assert_eq!(
        as_script.entries[0].origin,
        ContributorOrigin::FileScopeInterface
    );

    publish(
        &store,
        "/script.ts",
        "export {};\ninterface Window { fromScript: true }\n",
    );
    let as_module = lookup_global(&store, "Window");
    assert!(
        as_module.entries.is_empty(),
        "a module's top-level interface is not a global contributor"
    );
}

#[test]
fn classify_export_empty_as_module_and_bare_interface_as_script() {
    let script = ShallowFileState::service_backed_for_test("interface W { x: 1 }\n");
    let script_src: Arc<str> = Arc::from("interface W { x: 1 }\n");
    let script_indexed = IndexedReady::new_for_test_with_state(
        script.whole_hash,
        script,
        Arc::clone(&script_src),
        script_src,
    );
    assert_eq!(
        classify_module_kind(&script_indexed),
        FileModuleKind::Script
    );

    let module_src = "export {};\ninterface W { x: 1 }\n";
    let module = ShallowFileState::service_backed_for_test(module_src);
    let module_src: Arc<str> = Arc::from(module_src);
    let module_indexed = IndexedReady::new_for_test_with_state(
        module.whole_hash,
        module,
        Arc::clone(&module_src),
        module_src,
    );
    assert_eq!(
        classify_module_kind(&module_indexed),
        FileModuleKind::Module
    );
}

#[test]
fn no_lib_excludes_automatic_lib_files_only() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "ambient:/t/lib.es5.d.ts",
        "interface Array<T> { length: number }\n",
    );
    publish(
        &store,
        "/user.d.ts",
        "export {};\ndeclare global { interface Array<T> { user: true } }\n",
    );
    assert!(is_automatic_lib_canonical("ambient:/t/lib.es5.d.ts"));
    assert!(!is_automatic_lib_canonical("/user.d.ts"));
    assert!(!is_automatic_lib_canonical("/src/lib.helpers.d.ts"));

    let with_libs = store.global_contributor_index().snapshot().lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "Array",
        None,
        true,
    );
    assert_eq!(with_libs.entries.len(), 2);

    let no_lib = store.global_contributor_index().snapshot().lookup(
        &AugmentationTargetKind::GlobalAugmentation,
        "Array",
        None,
        false,
    );
    assert_eq!(no_lib.entries.len(), 1);
    assert_eq!(no_lib.entries[0].origin, ContributorOrigin::DeclareGlobal);
    assert_ne!(no_lib.fingerprint, with_libs.fingerprint);
}

#[test]
fn cancelled_publication_does_not_install_a_stale_positive() {
    let store = FileArtifactStore::new();
    let index = store.global_contributor_index();
    index.publish(1, || 2);
    let snap = index.snapshot();
    assert_eq!(
        snap.revision, 0,
        "a publication whose membership epoch moved must be abandoned"
    );
    assert!(lookup_global(&store, "Window").entries.is_empty());
}

#[test]
fn global_tag_is_the_declare_global_sentinel() {
    assert_eq!(GLOBAL_AUGMENTATION_TAG, "$global");
}

fn classify(source: &str) -> FileModuleKind {
    let state = ShallowFileState::service_backed_for_test(source);
    let src: Arc<str> = Arc::from(source);
    let indexed =
        IndexedReady::new_for_test_with_state(state.whole_hash, state, Arc::clone(&src), src);
    classify_module_kind(&indexed)
}

#[test]
fn nested_namespace_export_is_a_script() {
    assert_eq!(
        classify("namespace N { export interface X {} }\n"),
        FileModuleKind::Script
    );
}

#[test]
fn dynamic_import_does_not_make_a_module() {
    assert_eq!(
        classify("import(\"./lazy\");\ninterface Window { x: 1 }\n"),
        FileModuleKind::Script
    );
}

#[test]
fn trailing_export_empty_makes_a_module() {
    assert_eq!(
        classify("interface Window { x: 1 }\nexport {};\n"),
        FileModuleKind::Module
    );
}

#[test]
fn division_slash_does_not_hide_trailing_export_empty() {
    assert_eq!(classify("foo / bar; export {};\n"), FileModuleKind::Module);
    assert_eq!(
        classify("const x = /export/;\ninterface W { x: 1 }\n"),
        FileModuleKind::Script
    );
    assert_eq!(
        classify("const x = /foo/; export {};\n"),
        FileModuleKind::Module
    );
}

#[test]
fn same_canonical_replacement_keeps_one_live_version() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/global.d.ts",
        "export {};\ndeclare global { interface Function { a: 1 } }\n",
    );
    publish(
        &store,
        "/global.d.ts",
        "export {};\ndeclare global { interface Function { b: 2 } }\n",
    );
    assert_eq!(lookup_global(&store, "Function").entries.len(), 1);
}

#[test]
fn type_and_namespace_spaces_have_distinct_fingerprints() {
    let store = FileArtifactStore::new();
    publish(
        &store,
        "/script.ts",
        "interface N { x: 1 }\nnamespace N { export const v = 1; }\n",
    );
    let types = store.global_contributor_index().snapshot().lookup_in_space(
        &AugmentationTargetKind::GlobalAugmentation,
        "N",
        None,
        true,
        verter_semantic::facts::SymbolSpace::Type,
    );
    let namespaces = store.global_contributor_index().snapshot().lookup_in_space(
        &AugmentationTargetKind::GlobalAugmentation,
        "N",
        None,
        true,
        verter_semantic::facts::SymbolSpace::Namespace,
    );
    assert_eq!(types.entries.len(), 1);
    assert_eq!(namespaces.entries.len(), 1);
    assert_ne!(types.fingerprint, namespaces.fingerprint);
}
