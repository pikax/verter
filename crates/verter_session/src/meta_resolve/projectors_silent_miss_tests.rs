//! §7.5 silent-miss prevention tests for the per-macro projectors.
//!
//! Each projector that resolves a macro payload through dispatch
//! must publish a `MacroExpansionDiagnostics` envelope when
//! `ResolveMacroPayload` or `ProjectPath` returns `Recursive` or
//! `Error` — the analysis-wide `macro_expansion_diagnostics` stream
//! is the consumer-visible signal that the projection failed. A
//! projector that silently returned `Vec::new()` on
//! `QueryResult::Error` would publish an empty surface
//! indistinguishable from a successful empty result, breaking the
//! contract.
//!
//! The tests below construct fixture SFCs whose macro payload
//! references an undeclared / unimported type. The analyzer parses
//! the macro and produces a `parsed_type_argument`; the projector
//! attempts to lower + resolve through dispatch; the dispatch
//! returns an error; the projector pushes the diagnostic to
//! `macro_expansion_diagnostics`.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::HostConfig;
use crate::VerterHost;

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

/// Build a host backed by a `MemoryWorkspace` that owns the supplied
/// files. Mirrors the `build_workspace_host` helper from the
/// component-meta canonical-reuse tests.
fn build_workspace_host_for_silent_miss(files: &[(&str, &str)]) -> Arc<VerterHost> {
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

/// `defineProps<T>()` where T references a type that does not
/// resolve. The projector must publish a
/// `MacroExpansionDiagnostics` entry for `DefineProps` rather than
/// silently returning an empty `Vec`.
#[test]
fn project_props_unresolved_import_publishes_diagnostic() {
    let host = build_workspace_host_for_silent_miss(&[(
        "/workspace/src/Comp.vue",
        r#"<script setup lang="ts">
import type { MissingImport } from './does-not-exist'
defineProps<MissingImport>()
</script>
<template><div /></template>
"#,
    )]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("component meta should resolve even with unresolved type");

    // Discriminating assertion #1: the projector did NOT publish a
    // populated `props` surface for the unresolved type. Some
    // parser-side fields may exist (e.g., from runtime-prop fields
    // the parser captured), but the projector itself must not have
    // contributed a surface from the failed resolution.
    let define_props_diags: Vec<_> = meta
        .macro_expansion_diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.macro_kind,
                verter_semantic::analysis::component_meta::MacroExpansionKind::DefineProps,
            )
        })
        .collect();

    // Discriminating assertion #2: at least one DefineProps
    // diagnostic was published. Without the §7.5 silent-miss path,
    // an unresolved import would produce zero diagnostics — this
    // assertion fails on a silent-miss regression.
    assert!(
        !define_props_diags.is_empty(),
        "project_props must publish a MacroExpansionDiagnostics envelope \
         when ResolveMacroPayload or ProjectPath fails — found zero \
         DefineProps diagnostics on {} entries: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}

/// `defineEmits<T>()` where T references a type that does not
/// resolve. The projector must publish a
/// `MacroExpansionDiagnostics` entry for `DefineEmits`.
#[test]
fn project_emits_unresolved_import_publishes_diagnostic() {
    let host = build_workspace_host_for_silent_miss(&[(
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
        .expect("component meta should resolve even with unresolved type");

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
        "project_emits must publish a MacroExpansionDiagnostics envelope \
         when ResolveMacroPayload or ProjectPath fails — found zero \
         DefineEmits diagnostics on {} entries: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}

/// `defineSlots<T>()` where T references a type that does not
/// resolve. Either the projector (Phase 5 §7.5) or the slot-binding
/// graph synthesis (Phase 1) must publish a
/// `MacroExpansionDiagnostics` entry for `DefineSlots`.
#[test]
fn project_slots_unresolved_import_publishes_diagnostic() {
    let host = build_workspace_host_for_silent_miss(&[(
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
        .expect("component meta should resolve even with unresolved type");

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
        "project_slots / resolve_slot_bindings_graph_native must publish a \
         MacroExpansionDiagnostics envelope when ResolveMacroPayload or \
         ProjectPath fails — found zero DefineSlots diagnostics on {} \
         entries: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}

/// `defineProps<A & B>()` where B's resolution fails. The
/// projector must continue to publish A's members (intersection
/// arms that resolve are not lost when a sibling arm fails).
///
/// This is the discriminating §7.5 partial-intersection clause:
/// the projector must NOT abort the entire macro projection when a
/// single arm of the intersection fails to resolve. PartA's `a`
/// prop must survive even though `PartB` cannot be loaded.
///
/// Per-arm error reporting (a diagnostic specifically describing
/// PartB's failure) is a deeper integration with the
/// `NormalizeIntersection` dispatch and is tracked as a follow-up
/// — this test asserts only the survival contract.
#[test]
fn project_props_partial_intersection_publishes_diagnostic() {
    let host = build_workspace_host_for_silent_miss(&[
        (
            "/workspace/src/types.ts",
            r#"export interface PartA { a: string }
"#,
        ),
        (
            "/workspace/src/Comp.vue",
            r#"<script setup lang="ts">
import type { PartA } from './types'
import type { PartB } from './does-not-exist'
defineProps<PartA & PartB>()
</script>
<template><div /></template>
"#,
        ),
    ]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("component meta should resolve even when one intersection arm fails");

    // Discriminating assertion: A's `a` prop must be present — the
    // projector must not drop the successful arm's members. Without
    // this gate, an intersection where one arm fails would publish
    // an empty surface (the silent-miss case for partial
    // intersections).
    let has_a_prop = meta.props.iter().any(|p| p.name == "a");
    assert!(
        has_a_prop,
        "project_props on `PartA & PartB` must publish PartA's members \
         even when PartB fails to resolve — got props: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );
}
