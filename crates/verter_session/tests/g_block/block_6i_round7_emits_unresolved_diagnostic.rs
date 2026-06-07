//! Discriminator: **emits unresolved-import diagnostic preservation**.
//!
//! Regression guard: the macro-payload boundary MUST publish a
//! `MacroExpansionDiagnostics` envelope when
//! `defineEmits<MissingImport>()`'s payload fails to resolve.
//!
//! ## Why this guards the behaviour
//!
//! A publication path that lowers the macro payload via
//! `ProjectionMode::Navigate` fails loudly on an unresolved import:
//! `lower_type_expr_in_scope_with_mode` returns `None`, then
//! `resolve_macro_payload` pushes `macro-payload-lowering-failed`
//! into `diag_sink`. The diagnostic envelope lands.
//!
//! Under `structural_transit_with_mode(Navigate)` lowering, an
//! unresolved import may resolve to a `DeclRef` carrier
//! WITHOUT firing `walker_diagnostics` (the carrier-stop substrate
//! silently passes the missing decl through). To keep this contract
//! green, `resolve_macro_payload` uses the **macro-payload diagnostic
//! probe** that re-runs the payload resolution under publication
//! demand without publishing a value — purely for diagnostic
//! capture. The probe translates `Error`, `Recursive`, `Opaque`,
//! `cache_suppress`, and `walker_diagnostics` into the
//! `MacroExpansionDiagnostics` envelope so the unresolved-import
//! diagnostic contract holds under transit-shallow lowering.
//!
//! ## Discrimination
//!
//! `resolve_macro_payload` lowers under StructuralTransit AND invokes
//! the probe; the probe fires the diagnostic. A regression that drops
//! the probe / drops the diagnostic translation fails this test loudly.

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
fn round7_define_emits_unresolved_import_publishes_diagnostic_through_transit() {
    let host = build_workspace_host(&[(
        "/workspace/src/Comp.vue",
        r#"<script setup lang="ts">
import type { MissingEmits } from './does-not-exist'
defineEmits<MissingEmits>()
</script>
<template><div /></template>
"#,
    )]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("component meta should resolve even when emit type is unresolved");

    let define_emits_diags: Vec<_> = meta
        .macro_expansion_diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.macro_kind,
                verter_semantic::analysis::component_meta::MacroExpansionKind::DefineEmits,
            )
        })
        .collect();

    assert!(
        !define_emits_diags.is_empty(),
        "`defineEmits<MissingEmits>()` MUST publish a \
         `MacroExpansionDiagnostics` envelope with `macro_kind == DefineEmits`, \
         either via the Navigate-lowering failure path or via the \
         macro-payload diagnostic probe. A regression here means the probe was \
         not wired in or the silent-miss contract is not preserved. \
         Got {} total diagnostics: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}
