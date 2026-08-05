//! `ReturnType<typeof callee>` projector-admission acceptance suite.
//!
//! Pins the user-visible half of the flow-return substrate: a published
//! component-meta prop written as `ReturnType<typeof myType>['b']`
//! resolves the demanded member THROUGH the shared `FlowReturn` dispatch
//! path-precisely — the demanded member (`b`) materialises, the sibling
//! (`a`) stays unloaded (its cross-file value type is never ROUTED into a
//! semantic dispatch), and the publication pipeline records ZERO
//! `Published(Expanded)` projection contexts (Component-Meta
//! Shallow-By-Default).
//!
//! Non-materialization is asserted at the SEMANTIC-DISPATCH boundary, not
//! at the parse boundary. Shallow file processing indexes every reachable
//! import up front by design (Shallow File Processing Core Invariant), so
//! the sibling's FILE is legitimately read and indexed; what must never
//! happen is its declaration being deepened. `ProjectSemanticDispatch`
//! runs on the requesting thread, so the capture token observes its key
//! log directly — the positive control below proves the log is live
//! rather than trivially empty.
//!
//! Fixtures are vendored-in-memory only (Testing-Hermeticity).

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::{CaptureToken, DispatchEntry};
use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticQueryKey};
use crate::types::HostConfig;
use crate::VerterHost;
use verter_type_expr::TypeExpr;

#[allow(deprecated)]
fn make_workspace_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
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

/// Hermetic host over a `MemoryWorkspace` with a configured project.
fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let project_config = make_workspace_project_config("/workspace");
    #[allow(deprecated)]
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        project_config.clone(),
    ]));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ide_project = project_config.to_ide_project_config();
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    );
    host.configure_projects(vec![ide_project]);
    Arc::new(host)
}

/// The elided sibling's cross-file value: importing/resolving it is the
/// discriminating signal that the sibling was materialised.
const SIDE_TS: &str = r#"export interface SideShape {
  tag: string;
}
export declare const sideValue: SideShape;
"#;

/// The SFC under test: a script-setup-local function whose return object
/// carries the demanded member `b` (a fresh literal — widens to `number`)
/// and the sibling `a` whose value roots in the imported `sideValue`.
/// The published prop walks ONLY `['b']`.
const MYTYPE_SFC: &str = r#"<script setup lang="ts">
import { sideValue } from './side';
function myType() {
  const b = 1;
  return { a: sideValue, b };
}
defineProps<{ b: ReturnType<typeof myType>['b'] }>();
</script>
<template><div /></template>
"#;

/// A dispatched key's `Published(Expanded)` projection context, if any
/// (mirrors the publication-demand suite's classifier over the
/// context-bearing families).
fn key_published_expanded(key: &SemanticQueryKey) -> bool {
    let ctx: Option<ProjectionReductionContext> = match key {
        SemanticQueryKey::Instantiate(k) => Some(k.projection_reduction()),
        SemanticQueryKey::TypeOf { context, .. } => Some(context.projection_reduction),
        SemanticQueryKey::KeyOf { context, .. }
        | SemanticQueryKey::MappedType { context, .. }
        | SemanticQueryKey::ProjectPath { context, .. } => Some(*context),
        _ => None,
    };
    ctx.is_some_and(|c| {
        c.demand == crate::semantic_query::ReductionDemand::Published
            && c.mode == ProjectionMode::Expanded
    })
}

/// Debug-render of the offending dispatch keys touching the elided
/// sibling's file — the pass/fail signal for cross-file non-deepening.
fn sibling_touches(log: &[DispatchEntry]) -> Vec<String> {
    log.iter()
        .map(|entry| format!("{:?}", entry.key))
        .filter(|rendered| rendered.contains("/workspace/src/side.ts"))
        .collect()
}

/// `ReturnType<typeof myType>['b']` published through the projector
/// resolves `b` to the widened `number` — the flow-return substrate's
/// user-visible form.
#[test]
fn return_type_of_local_function_member_projects_widened_number() {
    let host = build_host(&[
        ("/workspace/src/side.ts", SIDE_TS),
        ("/workspace/src/MyType.vue", MYTYPE_SFC),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/MyType.vue")
        .expect("ReturnType-admission SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let b = meta
        .props
        .iter()
        .find(|p| p.name == "b")
        .expect("b prop present");
    let b_type = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/workspace/src/MyType.vue",
        b.type_source
            .present()
            .expect("b prop must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("b prop's published source must shell-materialize"));
    assert!(
        matches!(
            &b_type,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "ReturnType<typeof myType>['b'] must project the widened `number`, got {b_type:?}"
    );
}

/// Path-precision: the full `get_component_meta` for the
/// `ReturnType<typeof myType>['b']` prop loads ONLY `b` — the sibling
/// member's cross-file value never enters a semantic dispatch key — and
/// records ZERO `Published(Expanded)` projection contexts.
#[test]
fn return_type_member_demand_loads_only_the_walked_member() {
    let host = build_host(&[
        ("/workspace/src/side.ts", SIDE_TS),
        ("/workspace/src/MyType.vue", MYTYPE_SFC),
    ]);

    let guard = CaptureToken::start_for_query("flow_return_admission_guard");
    let resolved = host.get_component_meta_with_resolution("/workspace/src/MyType.vue");
    let snapshot = guard.end();
    let (_, resolution) = resolved.expect("ReturnType-admission SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    // POSITIVE CONTROL. The negative scans below are only evidence if the
    // dispatch log actually captured this request's semantic work. An
    // observation surface that silently captured nothing would pass every
    // "no key names the sibling" assertion while proving nothing, so the
    // log must be non-empty AND must name the owner the demand walks.
    assert!(
        !snapshot.dispatch_log.is_empty(),
        "the capture token observed ZERO dispatch keys — the negative scans below \
         would pass vacuously"
    );
    let owner_touches = snapshot
        .dispatch_log
        .iter()
        .filter(|entry| format!("{:?}", entry.key).contains("/workspace/src/MyType.vue"))
        .count();
    assert!(
        owner_touches > 0,
        "no dispatch key names the OWNER file — the log is live but not this request's"
    );

    // No dispatch key names the sibling's file: `sideValue`'s declaration
    // is never deepened, even though shallow indexing has seen its file.
    let touches = sibling_touches(&snapshot.dispatch_log);
    assert!(
        touches.is_empty(),
        "the elided sibling's file entered {} dispatch key(s):\n{}",
        touches.len(),
        touches.join("\n")
    );

    // Publication demand stays Navigate-only on the flow-return member
    // rail. ONE pre-existing exception is tolerated: the eval_env
    // macro-arg carrier-head lowering dispatches an EMPTY-path
    // `ProjectPath` at `Published(Expanded)` for a COMPOUND (non-reference)
    // payload root — a baseline behavior this fixture shape inherits
    // (verified present on the pre-slice tree), deliberately kept for
    // parent-generic instantiation of compound payload bodies and owned
    // by the eval_env expansion, not by this admission point. Every
    // OTHER `Published(Expanded)` key — a member walk, a `TypeOf`, an
    // `Instantiate` — would mean the flow member rail whole-materialised
    // and MUST stay zero.
    let expanded: Vec<String> = snapshot
        .dispatch_log
        .iter()
        .filter(|entry| key_published_expanded(&entry.key))
        .filter(|entry| {
            !matches!(
                &entry.key,
                SemanticQueryKey::ProjectPath { path, .. } if path.is_empty()
            )
        })
        .map(|entry| format!("{:?}", entry.key))
        .collect();
    assert!(
        expanded.is_empty(),
        "publication recorded {} `Published(Expanded)` projection context(s) beyond \
         the pre-existing empty-path carrier-head lowering:\n{}",
        expanded.len(),
        expanded.join("\n")
    );
}

/// Registry-live guard (`GuardId::NoFlowSlotInPublishedTypeSurface`) — a
/// `FlowReturn` whose intermediate solve transits flow slots publishes a
/// PLAIN reduced `TypeExpr` with no flow-internal identity. Two rails:
///
/// - STRUCTURAL: `TypeExpr` is a closed public enum with NO `FlowSlot`
///   variant — the published surface cannot carry a slot by type.
/// - BEHAVIORAL: the published prop reduces to the widened primitive
///   (`number`), and its rendered value carries no flow-internal
///   identity (`FlowSlot` / `FlowExpr` / `FlowSlice` / `SlotId`) — a
///   leaked opaque carrier would trip both assertions.
#[test]
pub(crate) fn no_flow_slot_in_published_type_surface() {
    let host = build_host(&[
        ("/workspace/src/side.ts", SIDE_TS),
        ("/workspace/src/MyType.vue", MYTYPE_SFC),
    ]);

    let (meta, resolution) = host
        .get_component_meta_with_resolution("/workspace/src/MyType.vue")
        .expect("ReturnType-admission SFC must resolve");
    assert!(!resolution.synthesis_should_suppress);

    let b = meta
        .props
        .iter()
        .find(|p| p.name == "b")
        .expect("b prop present");
    let b_type = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/workspace/src/MyType.vue",
        b.type_source
            .present()
            .expect("b prop must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("b prop's published source must shell-materialize"));
    assert!(
        matches!(
            &b_type,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the flow transit must REDUCE in the published surface, got {b_type:?}"
    );
    let rendered = format!("{b_type:?}");
    for leak in ["FlowSlot", "FlowExpr", "FlowSlice", "SlotId"] {
        assert!(
            !rendered.contains(leak),
            "published surface leaked a flow-internal identity `{leak}`: {rendered}"
        );
    }
}
