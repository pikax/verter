//! Silent-miss prevention for macro surface publication.
//!
//! A producer that cannot resolve an authored payload must retain its diagnostic
//! on the normalized bundle. Component metadata exposes that envelope even when
//! no members survive, and a warm bundle preserves the same diagnostics.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::HostConfig;
use crate::VerterHost;

#[test]
fn counted_emit_merge_owns_surface_without_an_enclosing_scope() {
    use verter_type_engine::project_semantic_dispatch::one_level_surface::project_one_level_surface;
    use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_engine::semantic_query::{
        DeclIdentity, LiteralValue, ProjectionMode, ProjectionReductionContext, SemanticNodeData,
        SurfaceEntry, SurfaceMember, SurfaceProvenanceContext, SurfaceView,
    };
    use verter_type_engine::semantic_query_memo::graph_fixture::GraphFixture;

    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::with_graph_fixture(&host, GraphFixture::new());
    let graph = dispatch.graph_arc();
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
    let surface = project_one_level_surface(
        &host,
        &dispatch,
        payload,
        Default::default(),
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
        None,
    );
    let owner = surface
        .recorded()
        .expect("both branches supply a complete surface");
    graph.invalidate_all();
    drop(input_roots);
    let members: Vec<_> = owner
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            SurfaceEntry::Member(member) => Some(member),
            _ => None,
        })
        .collect();
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
/// resolve. The producer must publish a
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

    // Discriminating assertion #1: the producer did NOT publish a
    // populated `props` surface for the unresolved type. Some
    // parser-side fields may exist (e.g., from runtime-prop fields
    // the parser captured), but the producer itself must not have
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
        "the macro surface producer must publish a MacroExpansionDiagnostics envelope \
         when ResolveMacroPayload or ProjectPath fails — found zero \
         DefineProps diagnostics on {} entries: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}

/// `defineEmits<T>()` where T references a type that does not
/// resolve. The producer must publish a
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
        "the macro surface producer must publish a MacroExpansionDiagnostics envelope \
         when ResolveMacroPayload or ProjectPath fails — found zero \
         DefineEmits diagnostics on {} entries: {:#?}",
        meta.macro_expansion_diagnostics.len(),
        meta.macro_expansion_diagnostics,
    );
}

/// `defineSlots<T>()` where T references a type that does not
/// resolve. Either the producer (the producer decomposition §7.5) or the slot-binding
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
        "the macro surface producer and slot bindings must publish a \
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
/// the producer must NOT abort the entire macro projection when a
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
        "the macro surface producer on `PartA & PartB` must publish PartA's members \
         even when PartB fails to resolve — got props: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );
}

#[test]
fn warm_macro_bundle_preserves_surface_diagnostics() {
    let host = build_workspace_host_for_silent_miss(&[(
        "/workspace/src/Comp.vue",
        r#"<script setup lang="ts">
defineProps<{ value: string } | number>()
</script>
<template><div /></template>"#,
    )]);
    assert!(host.ensure_loaded("/workspace/src/Comp.vue"));
    host.ensure_indexed_ready("/workspace/src/Comp.vue")
        .expect("the unchanged fixture is indexed before the bundle reads");
    let request = crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: Arc::from("/workspace/src/Comp.vue"),
        macro_index: 0,
        macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps,
        root_identity: [0; 16],
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    let cold = host.vue_macro_dtos(&request).expect("macro bundle");
    assert!(
        cold.diagnostics.iter().any(|envelope| envelope.diagnostics.iter().any(|diagnostic|
            diagnostic.reason == verter_session_query::analysis::type_expand::ExpansionStopReason::EmptyUnionArm)),
        "the non-object union arm carries its producer diagnostic: {:?}", cold.diagnostics
    );
    let warm = host
        .vue_macro_dtos(&request)
        .expect("unchanged macro bundle");
    assert_eq!(warm.diagnostics, cold.diagnostics);
    assert!(
        Arc::ptr_eq(&cold, &warm),
        "the second read must hit the stored bundle"
    );
    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("component meta");
    assert!(meta
        .macro_expansion_diagnostics
        .iter()
        .any(|diagnostic| diagnostic == &cold.diagnostics[0]));
}
