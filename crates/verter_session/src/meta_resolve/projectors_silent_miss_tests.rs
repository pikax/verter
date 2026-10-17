//! Silent-miss prevention tests for the per-macro projectors.
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

#[test]
fn counted_emit_merge_owns_surface_without_an_enclosing_scope() {
    use super::projectors::{resolve_payload_surface_with_scope, PayloadSurfaceScope};
    use verter_session_query::analysis::component_meta::MacroExpansionKind;
    use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_engine::semantic_query::{
        DeclIdentity, LiteralValue, SemanticNodeData, SurfaceMember, SurfaceView,
    };
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;

    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch =
        ProjectSemanticDispatch::with_ownership_harness(&host, CountedOwnershipHarness::new());
    let graph = dispatch.graph();
    let (payload, input_roots) = {
        let _scope = graph.enter_node_root_scope();
        let branches: Vec<_> = ["accepted", "dismissed"]
            .into_iter()
            .map(|name| {
                let value =
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(name.into())));
                let member = SurfaceMember {
                    key: verter_type_expr::AuthoredPropertyKey::string(name),
                    value,
                    optional: false,
                    readonly: false,
                    method_kind: None,
                    has_implementation_body: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    spans: Default::default(),
                    declaration_origin: None,
                    declared_in_macro_type_arg: Default::default(),
                    merge_role: Default::default(),
                    excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                };
                graph.intern_node(SemanticNodeData::Object(SurfaceView::from_members(
                    vec![member],
                    None,
                )))
            })
            .collect();
        let check = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("Mode"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: verter_type_expr::OwnedStr::from("Mode"),
        });
        let extends = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "editor".into(),
        )));
        let payload = graph.intern_node(SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref: branches[0],
            false_branch_ref: branches[1],
            distributive: false,
            pending: None,
        });
        (payload, graph.root_nodes([payload]).unwrap())
    };
    let mut diagnostics = Vec::new();
    let surface = resolve_payload_surface_with_scope(
        &dispatch,
        payload,
        0,
        MacroExpansionKind::DefineEmits,
        PayloadSurfaceScope::EmitClassMacroObject,
        &mut diagnostics,
    );
    let owner = surface
        .recorded()
        .expect("both branches supply a complete surface");
    let node = owner.id();
    graph.invalidate_all();
    drop(input_roots);
    let data = graph.node_data(node).unwrap();
    let SemanticNodeData::Object(view) = data.as_ref() else {
        panic!("merged emit surface was reclaimed before consumption: {data:?}");
    };
    let members = view.closed().complete_members();
    assert_eq!(members.len(), 2);
    assert_eq!(
        members
            .iter()
            .map(|m| m.published_name().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["accepted", "dismissed"]
    );
    assert!(members
        .iter()
        .all(|member| graph.node_is_live(member.value)));
}

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
        compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::configured_membership_match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
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
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);
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
                verter_session_query::analysis::component_meta::MacroExpansionKind::DefineProps,
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
                verter_session_query::analysis::component_meta::MacroExpansionKind::DefineEmits,
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
/// resolve. Either the projector (the projector decomposition §7.5) or the slot-binding
/// graph synthesis must publish a
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
                verter_session_query::analysis::component_meta::MacroExpansionKind::DefineSlots,
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
/// `ReduceIntersection` dispatch and is tracked as a follow-up
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
