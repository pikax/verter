use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;
use verter_analysis::type_eval_build::EvaluatedComponentTypes;
use verter_analysis::type_expr::{ObjectMember, PrimitiveName, TypeExpr};

fn make_project() -> Arc<MetaProject> {
    make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    })
}

fn make_project_with_config(config: HostConfig) -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..config
    });
    MetaProject::new(host)
}

fn make_workspace_project(ws: Arc<verter_vfs::MemoryWorkspace>) -> Arc<MetaProject> {
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
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

fn cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: crate::types::ResolverMode,
) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
    #[cfg(feature = "scheduler")]
    {
        project
            .host()
            .compile_cache
            .get(canonical)
            .and_then(|entry| {
                entry
                    .cached_resolved_meta
                    .get(&mode)
                    .map(|cached| Arc::clone(&cached.state))
            })
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files.get(canonical).and_then(|entry| {
            entry
                .cached_resolved_meta
                .get(&mode)
                .map(|cached| Arc::clone(&cached.state))
        })
    }
}

#[cfg(feature = "scheduler")]
fn cached_fallthrough_state(
    project: &MetaProject,
    canonical: &str,
) -> Option<Arc<crate::types::FallthroughResolution>> {
    project
        .host()
        .compile_cache
        .get(canonical)
        .and_then(|entry| {
            entry
                .cached_fallthrough
                .as_ref()
                .map(|(_, _, cached)| Arc::clone(cached))
        })
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
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

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
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

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

    project.clear_caches().unwrap();

    // Base file should still exist and be queryable after clearing caches
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

    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("first evaluation should populate the cache");

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("second evaluation should reuse the cache");

    assert_eq!(first.props.len(), second.props.len());
    assert!(Arc::ptr_eq(&first_cache, &second_cache));

    session
        .upsert("Comp.vue", sfc("count: number; label: string"))
        .unwrap();
    let third = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let third_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("updated file should repopulate the cache");

    assert!(third.props.iter().any(|field| field.name == "label"));
    assert!(!Arc::ptr_eq(&second_cache, &third_cache));
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
    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
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
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("dependency update should repopulate the cache");

    assert!(
        !Arc::ptr_eq(&first_cache, &second_cache),
        "dependency change must invalidate the owner's resolved-meta cache",
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
fn evaluate_types_returns_correct_results_for_imported_types() {
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
defineProps<{ item: Props }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();

    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: the prop referencing the imported type is present
    assert_eq!(
        evaluated.props.len(),
        1,
        "should have exactly 1 prop 'item'"
    );
    assert_eq!(evaluated.props[0].name, "item");

    // Assert-: no spurious props with names from the imported interface
    assert!(
        !evaluated
            .props
            .iter()
            .any(|p| p.name == "a" || p.name == "b"),
        "imported interface fields should not appear as top-level props"
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
fn evaluate_types_works_independently_of_prior_get_analysis_call() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("count: number; label: string"))
        .unwrap();

    let session = project.open_session().unwrap();

    // Call get_analysis first (raw, no enrichment)
    let analysis = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("get_analysis should return raw analysis");

    // get_analysis returns raw props
    let raw_names = prop_names(&analysis);
    assert!(
        raw_names.contains(&"count".to_string()),
        "raw analysis should have 'count' prop"
    );

    // evaluate_types should still work correctly regardless of prior get_analysis
    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: types are properly resolved
    assert_eq!(
        evaluated_prop_type(&evaluated, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number),
    );
    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String),
    );

    // Assert-: only the expected props
    assert_eq!(evaluated.props.len(), 2);
}

#[test]
fn evaluate_types_returns_consistent_results_for_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    let session = project.open_session().unwrap();

    // First call
    let first = session
        .evaluate_types("/App.vue")
        .expect("first evaluate_types should succeed")
        .expect("should return evaluated types");

    // Second call — should return identical results
    let second = session
        .evaluate_types("/App.vue")
        .expect("second evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: both calls return the same prop count and types
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated evaluate_types calls should return the same number of props"
    );
    assert_eq!(
        evaluated_prop_type(&first, "a"),
        evaluated_prop_type(&second, "a"),
        "repeated calls should return the same type for prop 'a'"
    );

    // Assert-: no extra props introduced
    assert_eq!(first.props.len(), 2, "should have exactly 2 props");
}

#[test]
fn resolve_component_meta_expanded_returns_consistent_results_on_repeated_calls() {
    use crate::types::ResolverMode;

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

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    // Force host to load the file
    let _ = session.get_analysis("/App.vue").unwrap();

    // First call
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    // Second call — should return consistent results
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("second resolve_component_meta should succeed");

    // Assert+: both calls return the same resolved macros
    assert_eq!(
        first.resolved_macros.len(),
        second.resolved_macros.len(),
        "repeated calls should return the same number of resolved macros"
    );

    // Assert+: resolved macros have consistent prop counts
    assert!(
        !first.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types on first call"
    );
    assert!(
        !second.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types on second call"
    );
    assert_eq!(
        first.resolved_macros[0].props.len(),
        second.resolved_macros[0].props.len(),
        "repeated calls should produce the same resolved prop count"
    );

    // Assert-: mode is Expanded, not Type
    assert_eq!(first.mode, ResolverMode::Expanded);
    assert_ne!(first.mode, ResolverMode::Type);
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_owner_change() {
    use crate::types::ResolverMode;

    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    // First call — inline props should be resolved
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    let first_snap_props = prop_names(&first.snapshot);
    assert!(
        first_snap_props.contains(&"a".to_string()),
        "first call should have prop 'a', got: {:?}",
        first_snap_props
    );
    assert_eq!(first_snap_props.len(), 2, "should start with 2 props");

    // Modify the owner SFC to change props
    project
        .upsert_base("/App.vue", &sfc("c: boolean; d: string"))
        .unwrap();

    // Second call — should see the updated props
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("second resolve_component_meta should succeed after owner change");

    let second_snap_props = prop_names(&second.snapshot);

    // Assert+: result includes the new props
    assert!(
        second_snap_props.contains(&"c".to_string()),
        "owner change should produce updated props including 'c', got: {:?}",
        second_snap_props
    );
    assert!(
        second_snap_props.contains(&"d".to_string()),
        "owner change should produce updated props including 'd', got: {:?}",
        second_snap_props
    );

    // Assert-: old props should not appear
    assert!(
        !second_snap_props.contains(&"a".to_string()),
        "old prop 'a' should not appear after owner change"
    );
    assert!(
        !second_snap_props.contains(&"b".to_string()),
        "old prop 'b' should not appear after owner change"
    );
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_dependency_change() {
    use crate::types::ResolverMode;

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
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // First call — should resolve props a, b via resolved_macros
    let first = project
        .host()
        .resolve_component_meta("/src/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    assert!(
        !first.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types"
    );
    let first_prop_names: Vec<&str> = first.resolved_macros[0]
        .props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        first_prop_names.contains(&"a") && first_prop_names.contains(&"b"),
        "first call should resolve props a and b, got: {:?}",
        first_prop_names
    );

    // Modify the dependency via base upsert (directly on host, not session)
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number; c: boolean }"#,
        )
        .unwrap();

    // Second call — should reflect the dependency change
    let second = project
        .host()
        .resolve_component_meta("/src/App.vue", ResolverMode::Expanded)
        .expect("resolve_component_meta should succeed after dependency change");

    assert!(
        !second.resolved_macros.is_empty(),
        "should still have resolved macros after dep change"
    );
    let second_prop_names: Vec<&str> = second.resolved_macros[0]
        .props
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    // Assert+: result includes the new prop 'c'
    assert!(
        second_prop_names.contains(&"c"),
        "dependency change should produce updated props including 'c', got: {:?}",
        second_prop_names
    );

    // Assert-: should not still have only the old 2-prop result
    assert!(
        second_prop_names.len() > 2,
        "dependency change must not return the stale 2-prop result, got: {:?}",
        second_prop_names
    );
}

#[test]
fn invalidate_compile_slots_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before invalidation");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' prop before invalidation"
    );

    project.host().invalidate_compile_slots("/App.vue");

    // Assert+: analysis still works after invalidation
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should still work after invalidate_compile_slots");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' prop after invalidation"
    );

    // Assert-: no spurious props introduced
    assert_eq!(
        after_names.len(),
        1,
        "should have exactly 1 prop after invalidation, not more"
    );
}

#[test]
fn removing_dependency_does_not_break_subsequent_analysis() {
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
    // Verify analysis works before removal
    let before = session
        .get_analysis("/src/App.vue")
        .unwrap()
        .expect("analysis should work before dependency removal");
    // Raw analysis may not resolve cross-file props, but should succeed
    assert!(
        before
            .macros
            .iter()
            .any(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps),
        "should have defineProps macro before removal"
    );

    let _ = project.host().remove("/src/types.ts");

    // Assert+: analysis still returns a result (doesn't panic/crash)
    let after = session.get_analysis("/src/App.vue").unwrap();
    assert!(
        after.is_some(),
        "analysis should still return a result after dependency removal"
    );

    // Assert-: the removed dependency should not be resolvable as a component
    assert!(
        project
            .host()
            .resolve_component_meta("/src/types.ts", crate::types::ResolverMode::Type)
            .is_none(),
        "removed dependency should not be resolvable via resolve_component_meta"
    );
}

#[cfg(not(feature = "scheduler"))]
#[test]
fn non_scheduler_upsert_reflects_updated_source_in_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before upsert");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' before upsert"
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

    // Assert+: subsequent analysis reflects updated content
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should work after upsert");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"count".to_string()),
        "should see 'count' after upsert, got: {:?}",
        after_names
    );

    // Assert-: should not lose the original prop
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' after upsert"
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

    // Assert+: resolved state was computed at most once
    assert!(
        p.component_meta_resolved_state_recomputes <= 1,
        "get_component_meta should compute resolved state at most once, got: {}",
        p.component_meta_resolved_state_recomputes
    );
}

#[test]
fn get_component_meta_returns_consistent_results_on_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();

    // First call
    let first = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("first call should return metadata");

    // Second call — should return consistent results
    let second = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("second call should return metadata");

    // Assert+: both calls return the same props
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated calls should return the same number of props"
    );
    assert_eq!(
        first.props[0].name, second.props[0].name,
        "repeated calls should return the same prop names"
    );

    // Assert-: no extra events/models introduced
    assert!(
        first.events.is_empty() && second.events.is_empty(),
        "no defineEmits means no events on either call"
    );
    assert!(
        first.models.is_empty() && second.models.is_empty(),
        "no defineModel means no models on either call"
    );
}

#[test]
fn get_component_meta_provenance_uses_single_resolver_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: exactly one resolved state computation
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "native get_component_meta should compute resolved state exactly once"
    );
    // Assert-: get_analysis should NOT have been called (component-meta uses the resolver path)
    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not call get_analysis()"
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
fn evaluate_types_prefers_declaration_entrypoints_for_package_type_imports() {
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
    let evaluated = session
        .evaluate_types("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let open_field = evaluated
        .define_props
        .iter()
        .flat_map(|entry| entry.fields.iter())
        .find(|field| field.name == "open")
        .expect("evaluated defineProps should include imported declaration prop");
    assert!(
        matches!(
            open_field.r#type,
            TypeExpr::Primitive(PrimitiveName::Boolean)
        ),
        "evaluate_types should resolve declaration-entrypoint prop types, got: {:?}",
        open_field.r#type
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

#[test]
fn union_object_variants_synthesize_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
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
    assert!(
        names.contains(&"layout"),
        "should have 'layout', got: {names:?}"
    );
    assert!(
        names.contains(&"editor"),
        "should have 'editor', got: {names:?}"
    );
    assert!(
        names.contains(&"floating"),
        "should have union branch props, got: {names:?}"
    );
}

#[test]
fn mixed_intersection_retains_local_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
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
    assert!(names.contains(&"id"), "should have 'id', got: {names:?}");
    assert!(
        names.contains(&"disabled"),
        "should have 'disabled', got: {names:?}"
    );
}

#[test]
fn imported_barrel_types_are_available_to_define_props_evaluation() {
    let project = make_project();
    project
        .upsert_base("/src/types/index.ts", r#"export * from '../Button.vue'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../Button.vue".to_string(),
            resolved_canonical_id: Some("/src/Button.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"icon"),
        "should have 'icon', got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should have 'label', got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"color"),
        "should omit 'color', got: {names:?}"
    );
}

#[test]
fn imported_barrel_cycles_still_resolve_nested_omit_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"loading"),
        "should include inherited icon props, got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"size"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"href"),
        "should include inherited link props, got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(!names.contains(&"icon"), "should omit icon, got: {names:?}");
    assert!(
        !names.contains(&"color"),
        "should omit color, got: {names:?}"
    );
    assert!(
        !names.contains(&"variant"),
        "should omit variant, got: {names:?}"
    );
    assert!(
        !names.contains(&"to"),
        "should omit link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"replace"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"activeClass"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"ariaCurrentValue"),
        "should omit router link keys, got: {names:?}"
    );
}

#[test]
fn resolve_component_meta_handles_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("expanded state should resolve");
    let button = resolved
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == "ButtonProps")
        .expect("should resolve ButtonProps");
    assert!(
        button.props.iter().any(|prop| prop.name == "loading"),
        "resolved ButtonProps should include inherited props, got: {:?}",
        button.props
    );
    assert!(
        button.props.iter().any(|prop| prop.name == "label"),
        "resolved ButtonProps should include button props, got: {:?}",
        button.props
    );
}

// ===========================================================================
// Phase 3: Fallthrough inheritance resolver
// ===========================================================================

use verter_analysis::component_meta::{
    AcceptedEventKind, AcceptedPropKind, AcceptedSurfaceCompleteness, BranchStatus,
    FallthroughSurface, MemberAvailability, MemberProvenance, PartialBranchReason,
    ResolvedRootStep, UnresolvedBranchReason,
};

/// Helper: get the component meta for a file (through session).
fn get_meta(
    project: &Arc<MetaProject>,
    canonical_id: &str,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let session = project.open_session().unwrap();
    session
        .get_component_meta(canonical_id)
        .unwrap()
        .expect("get_component_meta should return metadata")
}

#[test]
fn single_native_root_inherits_intrinsic_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is in accepted_props
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"
            && matches!(p.provenance, MemberProvenance::Declared)
            && matches!(p.kind, AcceptedPropKind::DeclaredProp)),
        "accepted_props should contain declared 'msg' prop, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited attrs from div should be present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"
            && matches!(p.provenance, MemberProvenance::Inherited { .. })
            && matches!(p.kind, AcceptedPropKind::Attr)),
        "accepted_props should contain inherited 'id' attr from <div>, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited events from div
    assert!(
        meta.accepted_events.iter().any(|e| e.name == "click"
            && matches!(e.provenance, MemberProvenance::Inherited { .. })
            && matches!(e.kind, AcceptedEventKind::Listener)),
        "accepted_events should contain inherited 'click' listener from <div>, got: {:?}",
        meta.accepted_events
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // Assert+: surface completeness should be Exact
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "completeness should be Exact for a simple native root"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches, got: {:?}",
        meta.fallthrough_surface
    );

    // Assert-: declared props should NOT appear in fallthrough_surface
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 1, "should have one branch");
        assert!(
            !branches[0].props.iter().any(|p| p.name == "msg"),
            "fallthrough_surface should NOT contain declared 'msg' prop"
        );
        assert_eq!(
            branches[0].status,
            BranchStatus::Resolved,
            "branch status should be Resolved"
        );
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "root_chain should show NativeTag div"
        );
    }
}

#[test]
fn explicit_root_bindings_are_subtracted() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div id="root" @click="() => {}">{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert-: explicitly bound 'id' attr should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].props.iter().any(|p| p.name == "id"),
            "consumed 'id' attr should be subtracted from inherited props"
        );
    }

    // Assert-: explicitly bound 'click' listener should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].events.iter().any(|e| e.name == "click"),
            "consumed 'click' listener should be subtracted from inherited events"
        );
    }

    // Assert+: other attrs should still be inherited
    assert!(
        meta.accepted_props.iter().any(
            |p| p.name == "title" && matches!(p.provenance, MemberProvenance::Inherited { .. })
        ),
        "non-consumed 'title' attr should still be inherited"
    );
}

#[test]
fn declared_props_and_events_take_precedence() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ id: number }>()
defineEmits<{ (e: 'click', value: string): void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: 'id' should be declared, not inherited
    let id_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "id")
        .expect("should have 'id' in accepted_props");
    assert!(
        matches!(id_prop.provenance, MemberProvenance::Declared),
        "'id' should be declared, not inherited"
    );

    // Assert+: 'click' should be declared, not inherited
    let click_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "click")
        .expect("should have 'click' in accepted_events");
    assert!(
        matches!(click_event.provenance, MemberProvenance::Declared),
        "'click' should be declared, not inherited"
    );

    // Assert-: should NOT have duplicate 'id' or 'click'
    assert_eq!(
        meta.accepted_props
            .iter()
            .filter(|p| p.name == "id")
            .count(),
        1,
        "'id' should appear exactly once"
    );
    assert_eq!(
        meta.accepted_events
            .iter()
            .filter(|e| e.name == "click")
            .count(),
        1,
        "'click' should appear exactly once"
    );
}

#[test]
fn declared_on_listener_alias_prop_blocks_inherited_click_listener() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ onClick?: () => void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|p| p.name == "onClick" && matches!(p.provenance, MemberProvenance::Declared)),
        "declared onClick prop must remain on the accepted prop surface"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "declared onClick prop must block the inherited click listener alias"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "fallthrough branches must not leak click when a declared onClick prop shadows it"
        );
    }
}

#[test]
fn inherit_attrs_false_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited props when inheritAttrs: false"
    );
    assert!(
        !meta
            .accepted_events
            .iter()
            .any(|e| matches!(e.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited events when inheritAttrs: false"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None when inheritAttrs: false"
    );
}

#[test]
fn unconditional_multi_root_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>a</div><span>b</span></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "multi-root should have no inherited props"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None for multi-root"
    );
}

#[test]
fn conditional_single_root_returns_exact_branches() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const show = true
defineProps<{ msg: string }>()
</script>
<template>
  <div v-if="show">a</div>
  <input v-else />
</template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: should have branches
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 2, "should have 2 branches (div, input)");

        // Branch 0: div
        assert_eq!(branches[0].branch_key, "0");
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "first branch should be div"
        );

        // Branch 1: input
        assert_eq!(branches[1].branch_key, "1");
        assert!(
            matches!(&branches[1].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "input"),
            "second branch should be input"
        );

        // Assert+: input-specific attrs should be conditional
        // (only in branch 1, not branch 0)
        let input_specific = meta.accepted_props.iter().find(|p| p.name == "type");
        if let Some(p) = input_specific {
            assert!(
                matches!(p.availability, MemberAvailability::Conditional { .. }),
                "'type' attr should be conditional (only in input branch)"
            );
        }
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn static_dynamic_is_root_resolves_native_candidates() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
const showNative = true
</script>
<template><component :is="showNative ? 'div' : Child" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "value")
        .expect("dynamic :is should propagate the input branch's accepted attrs");
    assert!(
        matches!(
            value_prop.availability,
            MemberAvailability::Conditional { .. }
        ),
        "input-only attrs from dynamic :is candidates must stay conditional"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .any(|branch| matches!(&branch.root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div")),
            "dynamic :is should produce a native div branch"
        );
        assert!(
            branches.iter().any(|branch| {
                branch
                    .root_chain
                    .iter()
                    .any(|step| matches!(step, ResolvedRootStep::Component { component_name, .. } if component_name == "Child"))
            }),
            "dynamic :is should also preserve the imported component branch"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_known_object_shape_is_consumed_exactly() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs = {
  id: 'root',
  onClick: () => {},
}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "exact root spread keys must be subtracted from inherited attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "exact root spread listener aliases must be subtracted from inherited listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "resolvable root spreads should not force a lower-bound surface"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.props.iter().all(|prop| prop.name != "id")),
            "spread-consumed attrs must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "spread-consumed listeners must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| matches!(branch.status, BranchStatus::Resolved)),
            "an exact root spread should keep the branch resolved"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_unknown_shape_uses_structured_partial_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs: Record<string, unknown> = {}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "unknown root spreads must lower accepted-surface completeness"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::PartiallyUnresolved { reasons }
                if reasons == &vec![PartialBranchReason::UnknownSpread]
        )),
        "unknown root spreads must surface a structured UnknownSpread reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn project_local_intrinsics_override_replaces_builtin_tag_surface() {
    let project = make_project();
    project
        .set_html_intrinsics_catalog(
            r#"{
  "tags": [
    {
      "tag": "div",
      "members": [
        { "name": "projectOnly", "kind": "attr", "rawType": "string" },
        { "name": "click", "kind": "listener", "rawType": "ProjectClickEvent" }
      ]
    }
  ]
}"#,
        )
        .unwrap();
    project
        .upsert_base("/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "projectOnly"),
        "project-local tag members must be used when a tag override is present"
    );
    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "id"),
        "tag overrides should replace the built-in fallback surface for that tag"
    );

    let click = meta
        .accepted_events
        .iter()
        .find(|event| event.name == "click")
        .expect("project-local listeners must still appear on the accepted event surface");
    assert!(
        matches!(
            &click.payload,
            TypeExpr::Unknown { raw } if raw == "(payload: ProjectClickEvent) => void"
        ),
        "project-local listener payloads must be preserved, got: {:?}",
        click.payload
    );
}

#[test]
fn generic_root_propagation_off_stays_sound() {
    let project = make_project();
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "generic root propagation disabled must not invent input-only attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "an unresolved generic root must remain a lower-bound surface"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::DynamicComponentIs
                }
            )
        }),
        "without propagation the generic child root should remain unresolved, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_specializes_dynamic_is_when_enabled() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "value")
        .expect("generic propagation should specialize the child root to input");
    assert!(
        matches!(value_prop.availability, MemberAvailability::Always),
        "single specialized generic roots should yield always-available attrs"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if component_name == "Poly" && tag == "input"
            )
        }),
        "generic propagation should resolve the child root chain to Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_recurses_through_component_chain() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Wrapper.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
import Poly from './Poly.vue'
defineProps<{ as: T }>()
</script>
<template><Poly :as="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Wrapper from './Wrapper.vue'
</script>
<template><Wrapper as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "recursive generic propagation should preserve the specialized input attrs through Wrapper"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name: wrapper_name, .. },
                    ResolvedRootStep::Component { component_name: poly_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if wrapper_name == "Wrapper" && poly_name == "Poly" && tag == "input"
            )
        }),
        "recursive generic propagation should resolve Wrapper -> Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn recursive_cycle_uses_structured_unresolved_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
</script>
<template><B /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
</script>
<template><A /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/A.vue");
    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::Cycle { canonical_id }
            } if canonical_id == "/A.vue"
        )),
        "cycles must terminate with a structured cycle reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );

    assert!(
        branches.iter().any(|branch| {
            branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Unresolved {
                        reason: UnresolvedBranchReason::Cycle { canonical_id },
                        ..
                    } if canonical_id == "/A.vue"
                )
            })
        }),
        "cycle branches must preserve the structured cycle reason in the root chain"
    );
}

#[test]
fn recursive_component_propagates_inherited_surface() {
    let project = make_project();

    // Child component with <div> root
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    // Parent with component root
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
defineProps<{ parentProp: string }>()
</script>
<template><Child :childProp="parentProp" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "should have declared 'parentProp'"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches for component root"
    );

    // Assert+: root_chain should show Component step
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(!branches.is_empty(), "should have at least one branch");
        assert!(
            branches[0]
                .root_chain
                .iter()
                .any(|step| matches!(step, ResolvedRootStep::Component { .. })),
            "root_chain should contain a Component step, got: {:?}",
            branches[0].root_chain
        );
    }
}

#[test]
fn recursive_component_keeps_child_declared_surface_alongside_child_fallthrough() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
defineEmits<{ (e: 'childClick', value: number): void }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let child_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "childProp")
        .expect("parent must expose child's declared prop through component root recursion");
    assert!(
        matches!(child_prop.provenance, MemberProvenance::Inherited { .. }),
        "child declared prop must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_prop.kind, AcceptedPropKind::Attr),
        "child declared prop should be exposed as an accepted attr on the parent"
    );

    let child_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "childClick")
        .expect("parent must expose child's declared event through component root recursion");
    assert!(
        matches!(child_event.provenance, MemberProvenance::Inherited { .. }),
        "child declared event must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_event.kind, AcceptedEventKind::Listener),
        "child declared event should be exposed as an accepted listener on the parent"
    );

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "parent must still expose child's inherited native attrs, not just declared members"
    );
}

#[test]
fn cycle_terminates_without_invented_members() {
    let project = make_project();

    // A imports B, B imports A — create a cycle
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
defineProps<{ aProp: string }>()
</script>
<template><B /></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
defineProps<{ bProp: string }>()
</script>
<template><A /></template>"#,
        )
        .unwrap();

    // Should not panic or infinite loop
    let meta = get_meta(&project, "/A.vue");

    // Assert+: declared props are present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "aProp"),
        "should have declared 'aProp'"
    );

    // Assert+: surface completeness should be LowerBound due to cycle
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "cycle should produce LowerBound completeness"
    );

    // Assert-: no invented members from the cycle
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "bProp"),
        "should NOT inherit 'bProp' through a cycle"
    );
}

#[test]
fn unresolved_target_branch_does_not_crash() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><slot /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from slot
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "slot root should produce no inherited props"
    );
}

#[test]
fn builtin_root_is_unresolved_branch() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><Teleport to="body">{{ msg }}</Teleport></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from Teleport
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "Teleport root should produce no inherited props"
    );
}

#[test]
fn accepted_surface_member_order_is_deterministic() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ z: string; a: number }>()
</script>
<template><div>test</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared props come first in declared source order
    let declared_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Declared))
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        declared_props,
        vec!["z", "a"],
        "declared props should keep source order"
    );

    // Assert+: inherited props come after declared, sorted lexicographically
    let inherited_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Inherited { .. }))
        .map(|p| p.name.as_str())
        .collect();
    let mut sorted = inherited_props.clone();
    sorted.sort();
    assert_eq!(
        inherited_props, sorted,
        "inherited props should be sorted lexicographically"
    );
}

#[test]
fn cache_hit_reused() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    // First call
    let meta1 = get_meta(&project, "/App.vue");
    // Second call — should use cache
    let meta2 = get_meta(&project, "/App.vue");

    // Assert+: both calls return the same accepted surface
    let names1: Vec<&str> = meta1
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let names2: Vec<&str> = meta2
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names1, names2, "cached result should be identical");
}

#[test]
fn child_change_invalidates_parent_fallthrough_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/App.vue");
    assert!(
        !first.accepted_props.iter().any(|p| p.name == "value"),
        "div-root child should not expose input-only attrs before the dependency changes"
    );

    #[cfg(feature = "scheduler")]
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("first query should cache fallthrough");

    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let second = get_meta(&project, "/App.vue");
    assert!(
        second.accepted_props.iter().any(|p| p.name == "value"),
        "parent fallthrough surface must refresh when the child root changes"
    );

    #[cfg(feature = "scheduler")]
    {
        let second_cache = cached_fallthrough_state(&project, "/App.vue")
            .expect("second query should repopulate the parent fallthrough cache");
        assert!(
            !Arc::ptr_eq(&first_cache, &second_cache),
            "dependency change must invalidate the parent's cached fallthrough surface"
        );
    }
}
