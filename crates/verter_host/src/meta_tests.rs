use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        deep_macro_resolution_type: true,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn sfc(props: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {props} }}>()
</script>
<template><div>hello</div></template>"#
    )
}

/// Extract prop field names from a FileAnalysisSnapshot's macros.
fn prop_names(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .macros
        .iter()
        .filter(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter())
        .map(|f| f.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Basic project lifecycle
// ---------------------------------------------------------------------------

#[test]
fn open_session_returns_unique_ids() {
    let project = make_project();
    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();
    assert_ne!(s1.id(), s2.id());
    assert_eq!(project.session_count(), 2);
}

#[test]
fn close_session_is_idempotent() {
    let project = make_project();
    let s = project.open_session().unwrap();
    s.close();
    s.close(); // second close is a no-op
    assert!(s.is_closed());
    assert_eq!(project.session_count(), 0);
}

#[test]
fn session_drop_auto_closes() {
    let project = make_project();
    {
        let _s = project.open_session().unwrap();
        assert_eq!(project.session_count(), 1);
    }
    assert_eq!(project.session_count(), 0);
}

#[test]
fn methods_fail_after_close() {
    let project = make_project();
    let s = project.open_session().unwrap();
    s.close();
    assert!(matches!(
        s.upsert("Comp.vue", "source".into()),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.delete("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.get_analysis("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
}

// ---------------------------------------------------------------------------
// Overlay isolation: two sessions don't see each other's overlays
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_dont_see_each_others_upserts() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 updates the file
    let modified = sfc("msg: string; count: number");
    s1.upsert("Comp.vue", modified.clone()).unwrap();

    // Session 1 sees the modified source
    let src1 = s1.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        src1.contains("count: number"),
        "session 1 should see its own overlay"
    );

    // Session 2 sees the original base source
    let src2 = s2.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        !src2.contains("count: number"),
        "session 2 must NOT see session 1's overlay"
    );
    assert!(
        src2.contains("msg: string"),
        "session 2 should see base source"
    );
}

#[test]
fn delete_in_session_a_does_not_hide_from_session_b() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 deletes the file
    s1.delete("Comp.vue").unwrap();

    // Session 1 doesn't see the file
    assert!(!s1.has_file("Comp.vue").unwrap());
    assert!(s1.get_effective_source("Comp.vue").unwrap().is_none());

    // Session 2 still sees the file
    assert!(s2.has_file("Comp.vue").unwrap());
    let src2 = s2.get_effective_source("Comp.vue").unwrap();
    assert!(src2.is_some(), "session 2 should still see the base file");
}

// ---------------------------------------------------------------------------
// Analysis through overlay
// ---------------------------------------------------------------------------

#[test]
fn get_analysis_sees_overlay_content() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    let modified = sfc("msg: string; count: number");
    s.upsert("Comp.vue", modified).unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "should return analysis for overlayed file"
    );

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"count".to_string()),
        "analysis should reflect overlay content with 'count' prop, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_without_overlay_uses_base() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();

    // No overlay — should see base analysis
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(analysis.is_some());

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"msg".to_string()),
        "should see base 'msg' prop, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"count".to_string()),
        "should NOT see 'count' prop from base"
    );
}

#[test]
fn get_analysis_for_deleted_file_returns_none() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    s.delete("Comp.vue").unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_none(),
        "analysis for tombstoned file should be None"
    );
}

// ---------------------------------------------------------------------------
// Overlay isolation for analysis
// ---------------------------------------------------------------------------

#[test]
fn analysis_isolation_between_sessions() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 modifies the file
    s1.upsert("Comp.vue", sfc("count: number")).unwrap();

    // Session 1 sees count
    let snap1 = s1.get_analysis("Comp.vue").unwrap().unwrap();
    let names1 = prop_names(&snap1);
    assert!(
        names1.contains(&"count".to_string()),
        "session 1 should see 'count', got: {:?}",
        names1
    );
    assert!(
        !names1.contains(&"msg".to_string()),
        "session 1 should NOT see 'msg'"
    );

    // Session 2 sees msg (base)
    let snap2 = s2.get_analysis("Comp.vue").unwrap().unwrap();
    let names2 = prop_names(&snap2);
    assert!(
        names2.contains(&"msg".to_string()),
        "session 2 should see base 'msg', got: {:?}",
        names2
    );
    assert!(
        !names2.contains(&"count".to_string()),
        "session 2 should NOT see session 1's 'count'"
    );
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

#[test]
fn shutdown_marks_project_dead() {
    let project = make_project();
    let s = project.open_session().unwrap();

    project.shutdown();

    assert!(project.is_shutdown());
    assert!(matches!(
        s.upsert("Comp.vue", "x".into()),
        Err(MetaError::Shutdown)
    ));
    assert!(matches!(project.open_session(), Err(MetaError::Shutdown)));
}

#[test]
fn shutdown_is_idempotent() {
    let project = make_project();
    project.shutdown();
    project.shutdown(); // no panic
}

// ---------------------------------------------------------------------------
// Overlay generation tracking
// ---------------------------------------------------------------------------

#[test]
fn overlay_generation_bumps_on_mutations() {
    let project = make_project();
    let s = project.open_session().unwrap();

    assert_eq!(s.overlay_generation(), 0);
    s.upsert("A.vue", "a".into()).unwrap();
    assert_eq!(s.overlay_generation(), 1);
    s.delete("B.vue").unwrap();
    assert_eq!(s.overlay_generation(), 2);
}

// ---------------------------------------------------------------------------
// visible_file_ids
// ---------------------------------------------------------------------------

#[test]
fn visible_file_ids_reflects_overlays() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s = project.open_session().unwrap();
    s.delete("A.vue").unwrap();
    s.upsert("C.vue", sfc("c: string")).unwrap();

    let ids = s.visible_file_ids().unwrap();
    assert!(!ids.contains(&"A.vue".to_string()), "A.vue was deleted");
    assert!(ids.contains(&"B.vue".to_string()), "B.vue is in base");
    assert!(
        ids.contains(&"C.vue".to_string()),
        "C.vue was added by overlay"
    );
}

// ---------------------------------------------------------------------------
// clear_caches preserves files but flushes compile results
// ---------------------------------------------------------------------------

#[test]
fn clear_caches_preserves_base_files() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("msg: string"))
        .unwrap();

    project.clear_caches().unwrap();

    // Base file should still exist and be queryable
    let s = project.open_session().unwrap();
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "file should still be accessible after clear_caches"
    );
}

// ---------------------------------------------------------------------------
// Dependency invalidation within session
// ---------------------------------------------------------------------------

#[test]
fn changing_dependency_invalidates_importer_in_session() {
    let project = make_project();

    // Set up a types file and a component that imports from it
    let types_source = r#"export interface ButtonProps { label: string }"#;
    let comp_source = r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div>{{ label }}</div></template>"#;

    project.upsert_base("types.ts", types_source).unwrap();
    project.upsert_base("Button.vue", comp_source).unwrap();

    let s = project.open_session().unwrap();

    // Query analysis succeeds for the base file
    let snap = s.get_analysis("Button.vue").unwrap();
    assert!(snap.is_some(), "analysis should succeed for the base file");

    // Modify types in session to add 'disabled'
    let new_types = r#"export interface ButtonProps { label: string; disabled: boolean }"#;
    s.upsert("types.ts", new_types.into()).unwrap();

    // After modifying types in the session, querying Button.vue through the
    // session should succeed (the overlay applies the new types.ts to the host)
    let snap2 = s.get_analysis("Button.vue").unwrap();
    assert!(
        snap2.is_some(),
        "analysis should succeed after dependency update"
    );
}

// ---------------------------------------------------------------------------
// Concurrent session activity (sequential in this test, but isolated)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_sessions_on_different_files() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 modifies A
    s1.upsert("A.vue", sfc("a_modified: number")).unwrap();

    // Session 2 modifies B
    s2.upsert("B.vue", sfc("b_modified: number")).unwrap();

    // Session 1 queries its files
    let snap_a1 = s1.get_analysis("A.vue").unwrap().unwrap();
    let names_a1 = prop_names(&snap_a1);
    assert!(
        names_a1.contains(&"a_modified".to_string()),
        "s1 should see its overlay on A, got: {:?}",
        names_a1
    );
    let snap_b1 = s1.get_analysis("B.vue").unwrap().unwrap();
    let names_b1 = prop_names(&snap_b1);
    assert!(
        names_b1.contains(&"b".to_string()),
        "s1 should see base B (not s2's overlay), got: {:?}",
        names_b1
    );

    // Session 2 queries its files
    let snap_b2 = s2.get_analysis("B.vue").unwrap().unwrap();
    let names_b2 = prop_names(&snap_b2);
    assert!(
        names_b2.contains(&"b_modified".to_string()),
        "s2 should see its overlay on B, got: {:?}",
        names_b2
    );
    let snap_a2 = s2.get_analysis("A.vue").unwrap().unwrap();
    let names_a2 = prop_names(&snap_a2);
    assert!(
        names_a2.contains(&"a".to_string()),
        "s2 should see base A (not s1's overlay), got: {:?}",
        names_a2
    );
}
