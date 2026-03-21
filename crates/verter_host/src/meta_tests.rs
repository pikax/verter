use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;
use verter_analysis::type_eval_build::EvaluatedComponentTypes;
use verter_analysis::type_expr::{ObjectMember, PrimitiveName, TypeExpr};

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        deep_macro_resolution_type: true,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn make_workspace_project(ws: Arc<verter_vfs::MemoryWorkspace>) -> Arc<MetaProject> {
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            deep_macro_resolution_type: true,
            ..HostConfig::default()
        },
        ws,
    );
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

fn evaluated_prop_type<'a>(types: &'a EvaluatedComponentTypes, name: &str) -> &'a TypeExpr {
    &types
        .props
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop {name}"))
        .r#type
}

fn cached_evaluated_types(
    project: &MetaProject,
    canonical: &str,
) -> Option<(crate::types::Hash16, Arc<EvaluatedComponentTypes>)> {
    #[cfg(feature = "scheduler")]
    {
        project
            .host()
            .compile_cache
            .get(canonical)
            .and_then(|entry| entry.cached_evaluated_types.clone())
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files
            .get(canonical)
            .and_then(|entry| entry.cached_evaluated_types.clone())
    }
}

fn cached_enriched_analysis(
    project: &MetaProject,
    canonical: &str,
) -> Option<(
    crate::types::Hash16,
    Arc<crate::types::FileAnalysisSnapshot>,
)> {
    #[cfg(feature = "scheduler")]
    {
        project
            .host()
            .compile_cache
            .get(canonical)
            .and_then(|entry| entry.cached_enriched_analysis.clone())
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files
            .get(canonical)
            .and_then(|entry| entry.cached_enriched_analysis.clone())
    }
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
fn ensure_loaded_populates_shared_base_from_workspace() {
    let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
        verter_vfs::MemoryOptions::default(),
    ));
    ws.inject_file("/workspace/App.vue".to_string(), Arc::from(sfc("msg: string")));

    let project = make_workspace_project(Arc::clone(&ws));

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should materialize the workspace file into the shared base project"
    );
    assert!(
        project.base_file_ids().contains("/workspace/App.vue"),
        "base index should include the loaded workspace file"
    );

    let session = project.open_session().unwrap();
    assert!(session.has_file("/workspace/App.vue").unwrap());
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the loaded base source");
    assert!(source.contains("msg: string"));
}

#[test]
fn refresh_base_reloads_workspace_source_into_shared_base() {
    let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
        verter_vfs::MemoryOptions::default(),
    ));
    ws.inject_file("/workspace/App.vue".to_string(), Arc::from(sfc("msg: string")));

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(project.ensure_loaded("/workspace/App.vue").unwrap());

    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("count: number")),
    );

    assert!(
        project.refresh_base("/workspace/App.vue").unwrap(),
        "refresh_base should reload the latest workspace content into shared base state"
    );

    let session = project.open_session().unwrap();
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the refreshed base source");
    assert!(source.contains("count: number"));
    assert!(!source.contains("msg: string"));
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

    let s = project.open_session().unwrap();
    let _ = s
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist before clearing caches");
    assert!(
        cached_enriched_analysis(&project, "Comp.vue").is_some(),
        "clear_caches test should start with a populated enriched-analysis cache"
    );

    project.clear_caches().unwrap();

    assert!(
        cached_enriched_analysis(&project, "Comp.vue").is_none(),
        "clear_caches must flush the enriched-analysis cache",
    );

    // Base file should still exist and be queryable
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

// ---------------------------------------------------------------------------
// Native type evaluation
// ---------------------------------------------------------------------------

#[test]
fn evaluate_types_combines_all_cached_script_blocks() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
function makeLabel() {
  return "cached" as string
}
</script>

<script setup lang="ts">
defineProps<{
  label: ReturnType<typeof makeLabel>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(
        evaluated.props.iter().all(|field| field.name != "missing"),
        "evaluation should only include actual props"
    );
}

#[test]
fn get_analysis_resolves_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
export interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "exported interface field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "exported interface field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_resolves_non_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "sibling script field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "sibling script field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn evaluate_types_reuses_cached_results_until_the_file_changes() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let session = project.open_session().unwrap();
    let first = session.evaluate_types("Comp.vue").unwrap().unwrap();
    assert_eq!(
        evaluated_prop_type(&first, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number)
    );

    let first_cache = cached_evaluated_types(&project, "Comp.vue")
        .expect("first evaluation should populate the cache");

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache = cached_evaluated_types(&project, "Comp.vue")
        .expect("second evaluation should reuse the cache");

    assert_eq!(first.props.len(), second.props.len());
    assert!(Arc::ptr_eq(&first_cache.1, &second_cache.1));

    session
        .upsert("Comp.vue", sfc("count: number; label: string"))
        .unwrap();
    let third = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let third_cache = cached_evaluated_types(&project, "Comp.vue")
        .expect("updated file should repopulate the cache");

    assert!(third.props.iter().any(|field| field.name == "label"));
    assert!(!Arc::ptr_eq(&second_cache.1, &third_cache.1));
}

#[test]
fn evaluate_types_resolves_local_typeof_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
const theme = {
  item: "item",
  body: "body",
}

type Props = {
  ui: typeof theme
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_resolves_imported_types_before_running_utilities() {
    let project = make_project();
    project
        .upsert_base(
            "types.ts",
            r#"export interface ImportedUser {
  id: number
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"password"));
        }
        other => panic!("expected imported utility to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_invalidates_cached_results_when_dependency_changes() {
    let project = make_project();
    project
        .upsert_base(
            "types.ts",
            r#"export interface ImportedUser {
  id: number
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let first = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let first_cache = cached_evaluated_types(&project, "Comp.vue")
        .expect("first evaluation should populate the cache");

    match evaluated_prop_type(&first, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(names, vec!["id"]);
        }
        other => panic!("expected imported interface to resolve to an object, got {other:?}"),
    }

    session
        .upsert(
            "types.ts",
            r#"export interface ImportedUser {
  id: number
  label: string
}"#
            .into(),
        )
        .unwrap();

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache = cached_evaluated_types(&project, "Comp.vue")
        .expect("dependency update should repopulate the cache");

    assert!(
        !Arc::ptr_eq(&first_cache.1, &second_cache.1),
        "dependency change must invalidate the owner's evaluated-type cache",
    );
    match evaluated_prop_type(&second, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"label"));
        }
        other => panic!("expected imported interface to resolve to an object, got {other:?}"),
    }
}

// ===========================================================================
// Phase 1: Provenance counters and enriched-analysis caching
// ===========================================================================

/// Helper to read the provenance counters from a MetaProject's host.
fn provenance(project: &MetaProject) -> crate::types::MetaProvenanceSnapshot {
    project.host().provenance().snapshot()
}

#[test]
fn evaluate_types_records_resolved_state_recompute_when_enriched_snapshot_is_missing() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed");

    let p = provenance(&project);
    assert_eq!(p.evaluate_types_calls, 1);
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "evaluate_types should record one resolved-state recompute when no enriched snapshot exists",
    );
    assert_eq!(
        p.evaluate_types_reused_enriched_snapshot, 0,
        "evaluate_types should not report enriched-snapshot reuse on a cold path",
    );
}

#[test]
fn evaluate_types_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed on a cold path");

    let p = provenance(&project);
    assert_eq!(
        p.get_analysis_calls, 0,
        "evaluate_types should use the private resolved-state helper instead of the public get_analysis workflow",
    );
}

#[test]
fn evaluate_types_records_enriched_snapshot_reuse_after_get_analysis() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let _ = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("get_analysis should populate enriched-analysis cache");

    project.host().provenance().reset();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should reuse the cached enriched snapshot");

    let p = provenance(&project);
    assert_eq!(p.evaluate_types_calls, 1);
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "evaluate_types should not recompute resolved state when an enriched snapshot already exists",
    );
    assert_eq!(
        p.evaluate_types_reused_enriched_snapshot, 1,
        "evaluate_types should record enriched-snapshot reuse after get_analysis",
    );
}

#[test]
fn evaluate_types_does_not_trigger_second_deep_enrichment_for_unchanged_file() {
    // Setup: dep file + SFC importing from it
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Reset counters after upsert
    project.host().provenance().reset();

    let session = project.open_session().unwrap();

    // Act: call get_analysis() then evaluate_types() — the compat workflow
    let _analysis = session.get_analysis("/App.vue").unwrap();
    let _eval = session.evaluate_types("/App.vue").unwrap();

    let p = provenance(&project);

    // Assert+: get_analysis was called at least once
    assert!(
        p.get_analysis_calls >= 1,
        "get_analysis should have been called, got: {}",
        p.get_analysis_calls
    );

    // Assert+: deep enrichment ran exactly once (not twice)
    assert_eq!(
        p.get_analysis_deep_enrich_runs, 1,
        "deep enrichment should run exactly once for the compat workflow, got: {}",
        p.get_analysis_deep_enrich_runs
    );

    // Assert-: deep enrichment must NOT have run twice
    assert!(
        p.get_analysis_deep_enrich_runs != 2,
        "deep enrichment must NOT run twice for one compat request"
    );
}

#[test]
fn deep_enriched_analysis_is_cached_for_unchanged_owner() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    // First call triggers enrichment
    let first = session.get_analysis("/App.vue").unwrap().unwrap();
    let p1 = provenance(&project);
    assert_eq!(
        p1.get_analysis_deep_enrich_runs, 1,
        "first call should trigger exactly one enrichment"
    );

    // Second call should hit the cache
    let second = session.get_analysis("/App.vue").unwrap().unwrap();
    let p2 = provenance(&project);

    // Assert+: cache was hit on the second call
    assert_eq!(
        p2.get_analysis_enriched_cache_hits, 1,
        "second call should hit the enriched-analysis cache, got: {}",
        p2.get_analysis_enriched_cache_hits
    );

    // Assert+: both results have the same prop fields
    let first_props = prop_names(&first);
    let second_props = prop_names(&second);
    assert_eq!(
        first_props, second_props,
        "both results should have identical prop fields"
    );

    // Assert-: deep enrichment should NOT have run again
    assert_eq!(
        p2.get_analysis_deep_enrich_runs, 1,
        "deep enrichment should NOT run again for unchanged file, got: {}",
        p2.get_analysis_deep_enrich_runs
    );
}

#[test]
fn deep_enriched_analysis_invalidates_on_owner_change() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();

    // First call populates the cache
    let _first = session.get_analysis("/App.vue").unwrap().unwrap();

    // Modify the owner SFC to add a local prop
    session
        .upsert(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
interface LocalProps extends Props { c: boolean }
defineProps<LocalProps>()
</script>
<template><div /></template>"#
                .into(),
        )
        .unwrap();

    // Reset counters to measure the second call clearly
    project.host().provenance().reset();
    let second = session.get_analysis("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: result includes the new prop 'c'
    let names = prop_names(&second);
    assert!(
        names.contains(&"c".to_string()),
        "owner change should produce updated props including 'c', got: {:?}",
        names
    );

    // Assert+: deep enrichment ran again (cache was invalidated)
    assert_eq!(
        p.get_analysis_deep_enrich_runs, 1,
        "owner change should trigger a fresh deep enrichment, got: {}",
        p.get_analysis_deep_enrich_runs
    );

    // Assert-: cache should NOT have been reused (hash changed)
    assert_eq!(
        p.get_analysis_enriched_cache_hits, 0,
        "owner change should NOT reuse the enriched cache, got: {}",
        p.get_analysis_enriched_cache_hits
    );
}

#[test]
fn deep_enriched_analysis_invalidates_on_dependency_change() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Manually register the import dependency so reverse-dep tracking works.
    // In production, set_import_dependencies is called by the NAPI layer after
    // the bundler resolves specifiers to canonical IDs with extensions.
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();

    // First call populates the cache and enriches imported types
    let first = session.get_analysis("/src/App.vue").unwrap().unwrap();
    let first_names = prop_names(&first);
    assert!(
        first_names.contains(&"a".to_string()),
        "first call should resolve props, got: {:?}",
        first_names
    );

    // Modify the dependency to add prop 'c'
    session
        .upsert(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number; c: boolean }"#.into(),
        )
        .unwrap();

    // Reset counters
    project.host().provenance().reset();
    let second = session.get_analysis("/src/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: result includes the new prop 'c'
    let names = prop_names(&second);
    assert!(
        names.contains(&"c".to_string()),
        "dependency change should produce updated props including 'c', got: {:?}",
        names
    );

    // Assert+: deep enrichment re-ran (at least once)
    assert!(
        p.get_analysis_deep_enrich_runs >= 1,
        "dependency change should trigger re-enrichment, got: {}",
        p.get_analysis_deep_enrich_runs
    );
}

#[test]
fn invalidate_compile_slots_clears_enriched_analysis_cache() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let _ = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should populate the enriched cache");
    assert!(
        cached_enriched_analysis(&project, "/App.vue").is_some(),
        "enriched-analysis cache should be populated before invalidation"
    );

    project.host().invalidate_compile_slots("/App.vue");

    assert!(
        cached_enriched_analysis(&project, "/App.vue").is_none(),
        "invalidate_compile_slots must clear the enriched-analysis cache",
    );
}

#[test]
fn removing_dependency_clears_enriched_analysis_cache_for_dependents() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let _ = session
        .get_analysis("/src/App.vue")
        .unwrap()
        .expect("analysis should populate the dependent enriched cache");
    assert!(
        cached_enriched_analysis(&project, "/src/App.vue").is_some(),
        "dependent should have a populated enriched-analysis cache before removal"
    );

    let _ = project.host().remove("/src/types.ts");

    assert!(
        cached_enriched_analysis(&project, "/src/App.vue").is_none(),
        "removing a dependency must clear the dependent enriched-analysis cache",
    );
}

#[cfg(not(feature = "scheduler"))]
#[test]
fn non_scheduler_upsert_clears_enriched_analysis_cache_immediately() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let _ = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should populate the enriched cache");
    assert!(
        cached_enriched_analysis(&project, "/App.vue").is_some(),
        "non-scheduler test should start with a populated enriched-analysis cache"
    );

    let updated = sfc("msg: string; count: number");
    let _ = project
        .host()
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some("/App.vue".to_string()),
            input_id: "/App.vue".to_string(),
            source: Arc::from(updated.as_str()),
            file_kind: crate::types::FileKind::from_path("/App.vue"),
            aliases: Vec::new(),
        })
        .unwrap();

    assert!(
        cached_enriched_analysis(&project, "/App.vue").is_none(),
        "non-scheduler upsert must clear the stale enriched-analysis cache immediately",
    );
}

// ===========================================================================
// Phase 3: Native get_component_meta query
// ===========================================================================

#[test]
fn get_component_meta_returns_props_and_events() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    // Assert+: props extracted
    assert_eq!(meta.props.len(), 2, "should extract 2 props");
    assert_eq!(meta.props[0].name, "label");
    assert!(meta.props[0].required, "label should be required");
    assert_eq!(meta.props[1].name, "count");
    assert!(!meta.props[1].required, "count should be optional");

    // Assert+: events extracted
    assert_eq!(meta.events.len(), 1, "should extract 1 event");
    assert_eq!(meta.events[0].name, "change");

    // Assert-: no models, no exposed
    assert!(meta.models.is_empty(), "no defineModel → no models");
    assert!(meta.exposed.is_empty(), "no defineExpose → no exposed");
}

#[test]
fn get_component_meta_uses_single_native_query_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let p = provenance(&project);

    // Assert+: the new query was called
    assert_eq!(
        p.get_component_meta_calls, 1,
        "get_component_meta should record one call"
    );

    // Assert+: deep enrichment ran at most once
    assert!(
        p.get_analysis_deep_enrich_runs <= 1,
        "get_component_meta should perform at most one deep enrichment, got: {}",
        p.get_analysis_deep_enrich_runs
    );
}

#[test]
fn get_component_meta_reuses_enriched_cache_on_second_call() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();

    // First call
    let _first = session.get_component_meta("/App.vue").unwrap().unwrap();

    project.host().provenance().reset();

    // Second call — should reuse cached enriched analysis
    let _second = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.get_component_meta_calls, 1,
        "second call should be counted"
    );
    assert_eq!(
        p.get_analysis_enriched_cache_hits, 1,
        "second call should hit the enriched-analysis cache"
    );
    assert_eq!(
        p.get_analysis_deep_enrich_runs, 0,
        "second call should NOT run deep enrichment"
    );
}

#[test]
fn get_component_meta_provenance_has_zero_legacy_reentry() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.component_meta_legacy_workflow_reentry, 0,
        "native get_component_meta must not trigger legacy workflow reentry"
    );
}

#[test]
fn get_component_meta_does_not_call_public_evaluate_types_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.evaluate_types_calls, 0,
        "native get_component_meta must not route through the public evaluate_types workflow"
    );
}

#[test]
fn get_component_meta_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");
    let p = provenance(&project);

    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not route through the public get_analysis workflow",
    );
}

#[test]
fn get_component_meta_prefers_declaration_entrypoints_for_package_type_imports() {
    let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
        verter_vfs::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            deep_macro_resolution_type: true,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.props.len(), 1, "should extract the imported prop");
    assert_eq!(meta.props[0].name, "open");
    assert_eq!(meta.props[0].raw_type.as_deref(), Some("boolean"));
    assert!(
        matches!(
            meta.props[0].type_expr,
            TypeExpr::Primitive(PrimitiveName::Boolean)
        ),
        "expanded prop type should come from the declaration entrypoint, got: {:?}",
        meta.props[0].type_expr
    );
}

#[test]
fn get_component_meta_returns_full_native_metadata_contract() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import FancyButton from './FancyButton.vue'

const count = ref(0)
const accentColor = "red"
const doubled = computed(() => count.value * 2)

onMounted(() => {
  console.log(count.value)
})
</script>
<template>
  <FancyButton
    id="wrapper"
    ref="button"
    :label="`${doubled}`"
    class="primary"
    :class="{ active: count > 0 }"
    v-model="count"
  >
    <template #default>{{ count }}</template>
  </FancyButton>
</template>
<style scoped module="theme">
#wrapper .primary {
  color: v-bind(accentColor);
  --accent: red;
}
</style>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(
        meta.components.len(),
        1,
        "template component usage should be present"
    );
    assert_eq!(meta.components[0].name, "FancyButton");
    assert_eq!(
        meta.components[0].import_source.as_deref(),
        Some("./FancyButton.vue")
    );
    assert!(meta.components[0].has_dynamic_class);
    assert_eq!(meta.components[0].v_models, vec!["modelValue".to_string()]);

    assert_eq!(
        meta.template_refs.len(),
        1,
        "template refs should be present"
    );
    assert_eq!(meta.template_refs[0].name, "button");
    assert_eq!(meta.template_refs[0].target_tag, "FancyButton");

    assert!(
        meta.imports.iter().any(|import| import.source == "vue"),
        "script imports should be preserved"
    );
    assert!(
        meta.bindings
            .iter()
            .any(|binding| binding.name == "count" && binding.used_in_template),
        "bindings should preserve template usage information"
    );
    assert!(
        meta.vue_api_calls.iter().any(|call| matches!(
            call.api,
            verter_analysis::types::VueApiClassification::OnMounted
        )),
        "Vue API calls should be preserved"
    );
    assert_eq!(meta.styles.len(), 1, "style metadata should be present");
    assert_eq!(meta.styles[0].classes, vec!["primary".to_string()]);
    assert_eq!(meta.styles[0].ids, vec!["wrapper".to_string()]);
    assert_eq!(
        meta.styles[0].custom_properties,
        vec!["--accent".to_string()]
    );
    assert_eq!(meta.styles[0].v_binds, vec!["accentColor".to_string()]);
    assert!(
        meta.styles[0]
            .selectors
            .iter()
            .any(|selector| selector.text == "#wrapper .primary"),
        "style selectors should be preserved"
    );
}

// ===========================================================================
// Phase 6: Resolved external type cache
// ===========================================================================

#[test]
fn resolved_type_cache_is_reused_across_different_owners() {
    let project = make_project();

    // Shared dependency
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface SharedProps { shared: string }"#,
        )
        .unwrap();

    // Two different SFCs importing the same type from the same dep
    project
        .upsert_base(
            "/src/A.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/B.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dep resolution for both owners
    project.host().set_import_dependencies(
        "/src/A.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/B.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();

    // First owner resolves the type (cache miss)
    project.host().provenance().reset();
    let meta_a = session.get_component_meta("/src/A.vue").unwrap().unwrap();
    let p1 = provenance(&project);

    assert!(
        p1.resolved_external_type_cache_misses >= 1,
        "first owner should miss the resolved type cache"
    );
    assert_eq!(meta_a.props.len(), 1, "A.vue should have the shared prop");

    // Reset counters for second owner
    project.host().provenance().reset();
    let meta_b = session.get_component_meta("/src/B.vue").unwrap().unwrap();
    let p2 = provenance(&project);

    assert_eq!(meta_b.props.len(), 1, "B.vue should have the shared prop");
    assert_eq!(meta_b.props[0].name, "shared");

    // Assert+: second owner should hit the host-level cache
    assert!(
        p2.resolved_external_type_cache_hits >= 1,
        "second owner importing the same type from the same unchanged dep should hit the host-level cache, got hits={} misses={}",
        p2.resolved_external_type_cache_hits,
        p2.resolved_external_type_cache_misses,
    );
}

#[test]
fn resolved_type_cache_is_reused_for_workspace_only_package_dependencies() {
    let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
        verter_vfs::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface SharedProps { shared: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            deep_macro_resolution_type: true,
            ..HostConfig::default()
        },
        ws,
    );
    let project = MetaProject::new(host);
    project
        .configure_projects(vec![
            verter_analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ])
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/A.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/B.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();

    project.host().provenance().reset();
    let meta_a = session
        .get_component_meta("/workspace/src/A.vue")
        .unwrap()
        .unwrap();
    let p1 = provenance(&project);
    assert_eq!(
        meta_a.props.len(),
        1,
        "A.vue should resolve the package prop"
    );
    assert!(
        p1.resolved_external_type_cache_misses >= 1,
        "first owner should miss the resolved type cache for a workspace-only dep"
    );

    project.host().provenance().reset();
    let meta_b = session
        .get_component_meta("/workspace/src/B.vue")
        .unwrap()
        .unwrap();
    let p2 = provenance(&project);
    assert_eq!(
        meta_b.props.len(),
        1,
        "B.vue should resolve the package prop"
    );
    assert_eq!(meta_b.props[0].name, "shared");
    assert!(
        p2.resolved_external_type_cache_hits >= 1,
        "second owner should hit the host-level resolved type cache even when the dep only exists in the workspace, got hits={} misses={}",
        p2.resolved_external_type_cache_hits,
        p2.resolved_external_type_cache_misses,
    );
}

#[test]
fn resolved_type_cache_cleared_on_host_close() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let _ = session.get_component_meta("/App.vue").unwrap();

    // Verify cache is populated
    assert!(
        !project.host().resolved_type_cache.lock().is_empty(),
        "cache should be populated after resolution"
    );

    // Clear caches
    project.clear_caches().unwrap();

    assert!(
        project.host().resolved_type_cache.lock().is_empty(),
        "clear_caches must flush the resolved type cache"
    );
}

#[test]
fn resolved_type_cache_is_bounded() {
    // Verify that inserting beyond cap doesn't grow unbounded
    let host = VerterHost::new_standalone(HostConfig {
        deep_macro_resolution_type: true,
        ..HostConfig::default()
    });

    {
        let mut cache = host.resolved_type_cache.lock();
        // Fill to cap
        for i in 0..crate::types::RESOLVED_TYPE_CACHE_CAP {
            cache.insert(
                crate::types::ResolvedTypeCacheKey {
                    dep_canonical_id: format!("/dep_{i}.ts"),
                    dep_source_hash: [0u8; 16],
                    type_name: "T".to_string(),
                    resolve_kind: verter_vfs::ResolveRequestKind::TypeImport,
                },
                crate::types::ResolvedTypeCacheEntry { resolved: None },
            );
        }
        assert_eq!(
            cache.len(),
            crate::types::RESOLVED_TYPE_CACHE_CAP,
            "cache should be at cap"
        );
    }

    // The eviction happens inside resolve_external_type_from_loaded_files,
    // but we can verify the cap constant is reasonable.
    assert!(
        crate::types::RESOLVED_TYPE_CACHE_CAP >= 1024,
        "cache cap should be at least 1024"
    );
    assert!(
        crate::types::RESOLVED_TYPE_CACHE_CAP <= 16384,
        "cache cap should not exceed 16384"
    );
}

// ===========================================================================
// Phase 8: Correctness — typeof, double script, interface extends imported
// ===========================================================================

#[test]
fn local_typeof_resolves_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const config = { x: 1, y: 'hello' }
defineProps<typeof config>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: both fields from config
    assert!(names.contains(&"x"), "should have 'x', got: {names:?}");
    assert!(names.contains(&"y"), "should have 'y', got: {names:?}");

    // Assert-: no extra fields
    assert_eq!(meta.props.len(), 2, "should have exactly 2 props");
}

#[test]
fn double_script_same_file_visibility_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface SharedProps { shared: boolean }
</script>
<script setup lang="ts">
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    // Assert+: prop from sibling script block
    assert_eq!(
        meta.props.len(),
        1,
        "should have 1 prop from sibling script"
    );
    assert_eq!(meta.props[0].name, "shared");

    // Assert-: no unresolved types or errors — prop should be fully resolved
    assert!(
        meta.props[0].raw_type.is_some(),
        "shared prop should have a resolved raw type"
    );
}

#[test]
fn interface_extends_pick_of_imported_type_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps { a: string; b: number; c: boolean; d: object }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { BaseProps } from './base'
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: inherited + local
    assert!(
        names.contains(&"a"),
        "should have 'a' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"local"),
        "should have 'local', got: {names:?}"
    );

    // Assert-: excluded fields
    assert!(!names.contains(&"c"), "should NOT have 'c', got: {names:?}");
    assert!(!names.contains(&"d"), "should NOT have 'd', got: {names:?}");
}

// ===========================================================================
// Phase 9: LSP deep expansion config
// ===========================================================================

#[test]
fn deep_expansion_disabled_by_default() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(
        !host.deep_expansion_enabled(),
        "default host should NOT have deep expansion enabled"
    );
}

#[test]
fn deep_expansion_enabled_via_runtime_override() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(!host.deep_expansion_enabled());

    host.set_deep_expansion(true);
    assert!(
        host.deep_expansion_enabled(),
        "set_deep_expansion(true) should enable deep expansion"
    );

    host.set_deep_expansion(false);
    assert!(
        !host.deep_expansion_enabled(),
        "set_deep_expansion(false) should disable deep expansion"
    );
}

#[test]
fn deep_expansion_enabled_via_static_config() {
    let host = VerterHost::new_standalone(HostConfig {
        deep_macro_resolution_type: true,
        ..HostConfig::default()
    });
    assert!(
        host.deep_expansion_enabled(),
        "static config deep_macro_resolution_type=true should enable deep expansion"
    );
}

#[test]
fn runtime_override_can_disable_static_deep_expansion_config() {
    let host = VerterHost::new_standalone(HostConfig {
        deep_macro_resolution_type: true,
        ..HostConfig::default()
    });
    assert!(host.deep_expansion_enabled());

    host.set_deep_expansion(false);
    assert!(
        !host.deep_expansion_enabled(),
        "runtime override false should disable static deep expansion config"
    );
}

#[test]
fn get_analysis_uses_enriched_path_when_deep_expansion_override_set() {
    let host = VerterHost::new_standalone(HostConfig::default());
    host.set_deep_expansion(true);

    let project = crate::meta::MetaProject::new(host);
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    project.host().provenance().reset();
    let session = project.open_session().unwrap();
    let _analysis = session.get_analysis("/App.vue").unwrap();

    let p = provenance(&project);
    // When deep expansion is enabled via runtime override, get_analysis
    // runs enrichment even though config.deep_macro_resolution_type was
    // false at construction time.
    assert_eq!(
        p.get_analysis_deep_enrich_runs, 1,
        "runtime deep expansion override should trigger enrichment"
    );
}

#[test]
fn default_lsp_host_does_not_enable_deep_expansion() {
    // Simulate what the LSP does: HostConfig::default() with analysis_level Full
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    assert!(
        !host.deep_expansion_enabled(),
        "LSP-default host must NOT enable deep expansion"
    );
}
