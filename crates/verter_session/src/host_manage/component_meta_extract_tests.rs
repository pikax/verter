//! Discriminating tests for the scope-aware
//! `resolve_ref_to_root_identity` lookup: identity is
//! `ResolvedRootIdentity { canonical_id, symbol_name }`, so the same name in
//! two scopes is not collapsed, and a local declaration shadows a same-name
//! import per JavaScript module scoping.

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

/// Discriminating test 4 — scope correctness.
///
/// SFC A imports `Helper` from a sibling file B. A also defines a
/// LOCAL `Helper` inside `defineProps<{ inner: Helper }>()`. The
/// local declaration shadows the import per JS module-scope rules.
///
/// The walker MUST distinguish the two `Helper` identities by
/// `ResolvedRootIdentity` — the local one keys on
/// `(/src/App.vue, "Helper")`, the imported one keys on
/// `(/src/b.ts, "Helper")`.
///
/// The deleted string-keyed walker collected `(String, usize)` pairs
/// by name only, so the two `Helper`s collided. The
/// `ResolvedRootIdentity`-keyed walker resolves each `Ref` to its
/// canonical identity and the two scopes stay distinct.
#[test]
fn macro_participation_distinguishes_local_helper_from_imported_helper() {
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

    let project = make_project();
    project
        .upsert_base(
            "/src/b.ts",
            r#"export interface Helper {
  fromB: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
// Local Helper shadows the import for the macro's type argument scope.
interface Helper {
  inner: number
}
defineProps<Helper>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Establish base host state by issuing a get_component_meta call.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should resolve through the local Helper, not the import");

    // Verify that resolve_ref_to_root_identity discriminates the two
    // Helper identities for the App.vue scope.
    let host = project.host();
    let local_identity =
        super::resolve_ref_to_root_identity_for_test(host, "/src/App.vue", "Helper")
            .expect("local Helper must resolve to a root identity");
    let imported_identity =
        super::resolve_ref_to_root_identity_for_test(host, "/src/b.ts", "Helper")
            .expect("imported Helper must resolve to a root identity in b.ts scope");

    assert_eq!(
        local_identity,
        ResolvedRootIdentity::new("/src/App.vue", "Helper"),
        "local Helper must key on App.vue, NOT b.ts"
    );
    assert_eq!(
        imported_identity,
        ResolvedRootIdentity::new("/src/b.ts", "Helper"),
        "imported Helper must key on b.ts"
    );
    assert_ne!(
        local_identity, imported_identity,
        "the two Helper identities MUST NOT collide — naive (String, usize) keying would have collapsed them"
    );
}
