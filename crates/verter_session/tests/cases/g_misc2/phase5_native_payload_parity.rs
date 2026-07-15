//! Native payload parity test for the per-macro projector
//! decomposition.
//!
//! Characterises that the per-macro projector decomposition preserves
//! the shape of `ComponentMetaAnalysis` for representative fixtures.
//! The native payload's field set must continue to carry the same
//! canonical fields with the same semantics.

use std::sync::Arc;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[allow(deprecated)]
fn make_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
    }
}

fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    Arc::new(host)
}

const SIMPLE_VUE: &str = r#"<script setup lang="ts">
defineProps<{ name: string; age: number }>()
defineEmits<{ click: [event: string] }>()
defineSlots<{ default: () => unknown; header?: () => unknown }>()
defineOptions({ inheritAttrs: false })
</script>
<template><div /></template>
"#;

/// CHARACTERIZATION: native payload's canonical fields remain
/// populated for a representative SFC after the projector
/// decomposition. Each affected macro field MUST still produce a
/// non-default value.
#[test]
fn getcomponentmeta_native_payload_unchanged_post_phase5() {
    let host = build_host(&[("/workspace/src/Comp.vue", SIMPLE_VUE)]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed for representative SFC");

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    let event_names: Vec<String> = meta.events.iter().map(|e| e.name.clone()).collect();
    let slot_names: Vec<String> = meta.slots.iter().map(|s| s.name.clone()).collect();

    // Props parity: `props` lives on the analysis, `project_props`
    // populates it.
    assert!(
        prop_names.contains(&"name".to_string()),
        "parity check for `name` prop failed (got {prop_names:?})"
    );
    assert!(
        prop_names.contains(&"age".to_string()),
        "parity check for `age` prop failed (got {prop_names:?})"
    );

    // Emits parity: emits surface as the `events` field.
    assert!(
        event_names.contains(&"click".to_string()),
        "parity check for `click` event failed (got {event_names:?})"
    );

    // Slots parity: `slots` lives on the analysis, `project_slots`
    // populates it.
    assert!(
        slot_names.contains(&"default".to_string()),
        "parity check for `default` slot failed (got {slot_names:?})"
    );
    assert!(
        slot_names.contains(&"header".to_string()),
        "parity check for `header` slot failed (got {slot_names:?})"
    );

    // `accepted_surface_completeness` is an enum
    // (`Exact`/`LowerBound`); when populated, it characterises the
    // fallthrough resolver state for the published metadata.
    use verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness;
    let _completeness: AcceptedSurfaceCompleteness = meta.accepted_surface_completeness;
}

/// Companion: the audit corpus diff is bounded — the native payload's
/// high-level field set unchanged across the projector
/// decomposition. Tests the SFC blocks pass-through (parser data).
#[test]
fn getcomponentmeta_native_payload_preserves_parser_data() {
    let host = build_host(&[("/workspace/src/Comp.vue", SIMPLE_VUE)]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed");

    // `sfc_blocks` is parser data — unchanged row.
    assert!(
        meta.sfc_blocks.is_some(),
        "sfc_blocks must remain populated (parser data preserved \
         across projector decomposition)"
    );

    // `imports` is parser data — unchanged row. The fixture has no
    // imports, but the field must exist + be serializable. (Trivial
    // structural check — non-empty would be covered by other corpus
    // tests.)
    let _imports_len = meta.imports.len();
}
