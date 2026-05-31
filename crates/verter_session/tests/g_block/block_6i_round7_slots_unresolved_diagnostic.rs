//! Block 6.i Round 7 — discriminator: **slots unresolved-import diagnostic preservation**.
//!
//! Companion to `block_6i_round7_emits_unresolved_diagnostic`. The
//! `defineSlots<MissingImport>()` macro must publish a
//! `MacroExpansionDiagnostics` envelope under every round-7 commit:
//! pre-cutover via the Navigate-lowering failure path,
//! post-cutover via the macro-payload diagnostic probe (codex Q2) and
//! the equivalent probe in
//! `slot_binding_graph::resolve_slot_bindings_graph_native`.
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate extensions):** PASS — Navigate
//!   lowering fails loudly; the existing diagnostic path emits the
//!   envelope. Regression guard.
//! - **Commit 2 (substrate extensions added, including the probe):**
//!   PASS — `resolve_macro_payload` and `resolve_slot_bindings_graph_native`
//!   switch to StructuralTransit lowering AND invoke the probe.
//! - **Commit 3 (atomic cutover):** PASS — consumer migrations land;
//!   the probe continues to fire the diagnostic.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

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
        membership: verter_workspace::ProjectMembership::MatchAll,
    }
}

fn build_workspace_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
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

#[test]
fn round7_define_slots_unresolved_import_publishes_diagnostic_through_cutover() {
    let host = build_workspace_host(&[(
        "/workspace/src/Comp.vue",
        r#"<script setup lang="ts">
import type { MissingSlots } from './does-not-exist'
defineSlots<MissingSlots>()
</script>
<template><div /></template>
"#,
    )]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("component meta should resolve even when slots type is unresolved");

    let define_slots_diags: Vec<_> = meta
        .macro_expansion_diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.macro_kind,
                verter_semantic::analysis::component_meta::MacroExpansionKind::DefineSlots,
            )
        })
        .collect();

    assert!(
        !define_slots_diags.is_empty(),
        "Block 6.i Round 7 — `defineSlots<MissingSlots>()` MUST publish a \
         `MacroExpansionDiagnostics` envelope with `macro_kind == DefineSlots` \
         under EVERY round-7 commit: pre-cutover via the Navigate-lowering \
         failure path, post-cutover via the macro-payload diagnostic probe \
         (codex Q2) on both `resolve_macro_payload` and \
         `resolve_slot_bindings_graph_native`. A regression here means the \
         probe was not wired into both paths or the cutover landed without \
         preserving the silent-miss contract. Got {} total diagnostics: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}
