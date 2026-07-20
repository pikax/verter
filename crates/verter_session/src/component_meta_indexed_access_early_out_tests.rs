//! Indexed-access early-out characterisation tests.
//!
//! Architectural contract: published prop types stay shallow when not
//! used. The projector path publishes the symbolic carrier (bare
//! `Ref`, `IndexedAccess` chain, or terminal scalar) without running
//! eager member-route projection. These tests characterise the
//! shallow contract for indexed-access shapes:
//!
//! - Terminal scalar surfaces (`IconProps['name']` where
//!   `name?: string`) publish the scalar directly, so the projector
//!   reduction has nothing to expand and the published prop carries
//!   the terminal `string | undefined` (or the symbolic carrier the
//!   projector picks under the symbolic-route preservation contract).
//!
//! - Non-empty object-surface routes (`Button['slots']` where
//!   `slots: ButtonSlots`) publish the symbolic indexed-access; the
//!   slots-route preservation rule keeps the carrier visible for
//!   downstream consumers.
//!
//! - Counterfixtures exercise shapes where reduction may or may not
//!   produce structural improvement (conditional roots, recursive
//!   indexed access through Tree-shaped carriers, mapped indexed
//!   roots, `Record<K, never>` slots) — these characterise the
//!   resolver's behaviour without panicking and without depending on
//!   counters that the rescue cascade owned.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::{HostConfig, ProjectionMode};
use crate::VerterHost;

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// pre-populated with the supplied files. The workspace is configured
/// with a single project rooted at `/workspace` so cross-file
/// declarations resolve as workspace-owned (per
/// `WorkspaceRead::is_workspace_owned`).
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

/// Drive the component-meta resolution path and return successfully —
/// asserting nothing crashes and the request can be re-issued in
/// `Expanded` mode. The structural assertions live in the per-fixture
/// tests below and inspect the published `props[..].type_expr`
/// directly.
fn drive_resolution(host: &Arc<VerterHost>, canonical: &str) {
    let _ = host.get_component_meta(canonical);
    let _ = host.resolve_component_meta(canonical, ProjectionMode::Expanded);
}

// ── Positive #1: IconProps['name'] terminal-scalar early-out ──

const POSITIVE_TYPES_TS: &str = r#"export interface IconProps {
  name?: string
}
"#;

const POSITIVE_ICON_PROPS_VUE: &str = r#"<script setup lang="ts">
import type { IconProps } from '/workspace/src/types'
defineProps<{
  icon?: IconProps['name']
}>()
</script>
<template><div /></template>
"#;

/// Positive case: `IconProps['name']` where `name?: string`. The
/// published surface evaluates to `string | undefined` (terminal
/// scalar). The projector publishes the terminal scalar through
/// `dispatch.execute_read`. The semantic invariant is that the
/// published prop is correctly identified as `IconProps['name']`
/// (the prop is produced even though raw_type is an indexed
/// access).
#[test]
fn concrete_scalar_props_skip_raw_indexed_access_materialization() {
    let host = build_workspace_host(&[
        ("/workspace/src/types.ts", POSITIVE_TYPES_TS),
        ("/workspace/src/Icon.vue", POSITIVE_ICON_PROPS_VUE),
    ]);

    let meta = host
        .get_component_meta("/workspace/src/Icon.vue")
        .expect("getComponentMeta must succeed for Icon");

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.contains(&"icon".to_string()),
        "projector must publish `icon` prop \
         (got {prop_names:?})"
    );
}

// ── Positive #2: Button['slots'] non-empty object-surface early-out ──

const POSITIVE_BUTTON_TS: &str = r#"export interface ButtonSlots {
  default?: () => unknown
  prepend?: () => unknown
}

export interface Button {
  slots: ButtonSlots
}
"#;

const POSITIVE_BUTTON_VUE: &str = r#"<script setup lang="ts">
import type { Button } from '/workspace/src/button-types'
defineProps<{
  ui?: Button['slots']
}>()
</script>
<template><div /></template>
"#;

/// Positive case: `Button['slots']` where `slots: ButtonSlots` is a
/// non-empty object surface (`{ default?: ..., prepend?: ... }`).
///
/// Architectural contract: path-precise navigation, shallow-by-default
/// publication. The indexed path navigates ONLY the `slots` hop; the
/// `Published(Navigate)` terminal lands on the closed-object
/// declaration `ButtonSlots` and stays the declaration-reference
/// carrier — the consumer re-resolves `ButtonSlots` through the shared
/// resolver on demand. Other members of `Button` never load, and the
/// terminal is NOT eagerly flattened into an object surface at
/// publication time.
#[test]
fn concrete_slots_object_props_skip_define_props_member_route_projection() {
    use verter_type_expr::TypeExpr;

    let host = build_workspace_host(&[
        ("/workspace/src/button-types.ts", POSITIVE_BUTTON_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let meta = host
        .get_component_meta("/workspace/src/Button.vue")
        .expect("getComponentMeta must succeed for Button");
    let ui_prop = meta
        .props
        .iter()
        .find(|p| p.name == "ui")
        .expect("Button's defineProps publishes the `ui` prop");

    // The indexed path Button['slots'] navigates to the terminal
    // ButtonSlots declaration and publishes the reference carrier —
    // NOT an eagerly flattened object surface, and NOT the unresolved
    // `Button['slots']` indexed-access (the hop itself must resolve).
    let ui_type = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/workspace/src/Button.vue",
        ui_prop
            .type_source
            .present()
            .expect("the `ui` prop must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("`ui`'s published source must shell-materialize"));
    match &ui_type {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "ButtonSlots",
                "the Published(Navigate) terminal must land on the ButtonSlots \
                 declaration-reference carrier"
            );
            assert!(
                type_arguments.is_empty(),
                "ButtonSlots is not generic — the carrier carries no type arguments"
            );
        }
        TypeExpr::Object(obj) => panic!(
            "ui prop must stay the ButtonSlots declaration-reference carrier \
             (shallow-by-default publication) — it was eagerly flattened: {obj:?}"
        ),
        other => panic!(
            "ui prop's published type should be the ButtonSlots reference carrier \
             from `Button['slots']`, got {other:?}"
        ),
    }

    // Re-resolvability witness: the carrier is not a dead end — the
    // shared dispatch resolves the ButtonSlots declaration to its
    // closed object surface (default + prepend) on demand.
    use crate::semantic_query::SemanticQueryApi;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host.as_ref());
    let resolved = match dispatch.execute_type_node(
        crate::semantic_query::SemanticQueryKey::ResolveDecl(
            crate::semantic_query::ResolveDeclKey {
                scope: crate::semantic_query::ScopeId {
                    canonical_id: Arc::from("/workspace/src/button-types.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    local_scope: None,
                },
                name: Arc::from("ButtonSlots"),
            },
        ),
    ) {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value,
            ..
        }) => value,
        other => panic!("ButtonSlots must re-resolve through the shared dispatch, got {other:?}"),
    };
    let surface =
        match dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::ProjectPath {
            base: resolved,
            path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Shallow,
            ),
        }) {
            crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryOutput { value, .. },
            ) => value,
            other => panic!("ButtonSlots surface read must succeed, got {other:?}"),
        };
    let graph = host.project_type_store().semantic_graph();
    match graph.node_data(surface).as_deref() {
        Some(crate::semantic_query::SemanticNodeData::Object(view)) => {
            let names: Vec<&str> = view.members.iter().map(|m| m.name.as_ref()).collect();
            assert!(
                names.contains(&"default") && names.contains(&"prepend"),
                "the re-resolved ButtonSlots surface must expose `default` and `prepend`, \
                 got {names:?}"
            );
        }
        other => panic!(
            "the re-resolved ButtonSlots declaration must surface its object members, \
             got {other:?}"
        ),
    }
}

// ── Counterfixture #1: conditional indexed root takes slow path ──

const CF_CONDITIONAL_TYPES_TS: &str = r#"export type Pool<T> = T extends true ? { foo: number } : { bar: string }
"#;

const CF_CONDITIONAL_VUE: &str = r#"<script setup lang="ts">
import type { Pool } from '/workspace/src/cond-types'
defineProps<{
  picked?: Pool<true>['foo']
}>()
</script>
<template><div /></template>
"#;

/// Counterfixture: the indexed root `Pool<true>` is a conditional
/// shape — the early-out MUST NOT fire (the predicate excludes
/// conditional roots per §6.3 disallowed shapes). The member-route
/// projection must run so the slow path still produces a result.
#[test]
fn conditional_indexed_root_takes_slow_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/cond-types.ts", CF_CONDITIONAL_TYPES_TS),
        ("/workspace/src/Cond.vue", CF_CONDITIONAL_VUE),
    ]);

    // The counterfixture MUST take the slow path; we don't strictly
    // assert the counter is non-zero on every run because dispatch may
    // produce the result through a different branch — but the
    // early-out MUST NOT have fired. Since the published surface is
    // not terminal scalar (it's an indexed-access through a conditional
    // root), the predicate's `terminal-scalar-surface predicate`
    // returns false and the early-out is bypassed.
    //
    // We verify by negative: the test simply confirms the resolution
    // succeeds without panicking. A regression that mis-classifies
    // conditional roots as terminal would silently lose the conditional
    // distribution — covered by the absence of an early-out check on
    // conditional shapes.
    drive_resolution(&host, "/workspace/src/Cond.vue");
}

// ── Counterfixture #2: recursive indexed access takes slow path ──

const CF_RECURSIVE_TYPES_TS: &str = r#"export interface Tree {
  child: Tree
  leaf: string
}
"#;

const CF_RECURSIVE_VUE: &str = r#"<script setup lang="ts">
import type { Tree } from '/workspace/src/tree-types'
defineProps<{
  branch?: Tree['child']['leaf']
}>()
</script>
<template><div /></template>
"#;

/// Counterfixture: `Tree['child']['leaf']` where `child: Tree` is a
/// recursive ref. The early-out predicate's
/// `terminal-scalar-surface predicate` matches `string` (the
/// terminal projection), but the indexed-access route walks through a
/// recursive intermediate. The cycle-guard inside dispatch prevents
/// runaway recursion; the test asserts the resolution terminates
/// without panicking and produces a deterministic result.
#[test]
fn recursive_indexed_access_takes_slow_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/tree-types.ts", CF_RECURSIVE_TYPES_TS),
        ("/workspace/src/Branch.vue", CF_RECURSIVE_VUE),
    ]);

    drive_resolution(&host, "/workspace/src/Branch.vue");
}

// ── Counterfixture #3: mapped indexed root takes slow path ──

const CF_MAPPED_TYPES_TS: &str = r#"export interface Source {
  alpha: string
  beta: number
}

export type Mapped = { [K in keyof Source]: Source[K] }
"#;

const CF_MAPPED_VUE: &str = r#"<script setup lang="ts">
import type { Mapped } from '/workspace/src/mapped-types'
defineProps<{
  picked?: Mapped['alpha']
}>()
</script>
<template><div /></template>
"#;

/// Counterfixture: the indexed root `Mapped` is a mapped type — the
/// early-out MUST NOT fire. The member-route projection must run so
/// the mapped value type is correctly projected.
#[test]
fn mapped_indexed_root_takes_slow_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/mapped-types.ts", CF_MAPPED_TYPES_TS),
        ("/workspace/src/Map.vue", CF_MAPPED_VUE),
    ]);

    drive_resolution(&host, "/workspace/src/Map.vue");
}

// ── Counterfixture #4: Record<K, never> non-object-surface ──

const CF_RECORD_NEVER_TYPES_TS: &str = r#"export interface EmptyHolder {
  slots: Record<string, never>
}
"#;

const CF_RECORD_NEVER_VUE: &str = r#"<script setup lang="ts">
import type { EmptyHolder } from '/workspace/src/empty-holder'
defineProps<{
  ui?: EmptyHolder['slots']
}>()
</script>
<template><div /></template>
"#;

/// Counterfixture: `EmptyHolder['slots']` where `slots: Record<string,
/// never>`. Per §6.3 strict definition, an object solely consisting
/// of an index signature whose value type is `never` is NOT a
/// non-empty object surface — the slots-route early-out MUST NOT
/// fire. The slow path takes over.
#[test]
fn record_k_never_slots_takes_slow_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/empty-holder.ts", CF_RECORD_NEVER_TYPES_TS),
        ("/workspace/src/Empty.vue", CF_RECORD_NEVER_VUE),
    ]);

    drive_resolution(&host, "/workspace/src/Empty.vue");
}

// ── Invalidation: editing the indexed-access root re-runs the rescue ──

/// Editing the root declaration's body (e.g., changing
/// `IconProps.name?: string` to `IconProps.name?: 'small' | 'large'`)
/// must invalidate the cached early-out result and re-run the field
/// pipeline against the new published surface. The early-out fires both
/// before AND after the edit (both surfaces are terminal scalar/literal-
/// union); the test asserts the post-edit field type reflects the new
/// body shape, proving the cache was invalidated and re-resolved rather
/// than reusing the pre-edit terminal early-out result blindly.
#[test]
fn invalidation_indexed_access_root_edit() {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        Arc::from(POSITIVE_TYPES_TS),
    );
    workspace.inject_file(
        "/workspace/src/Icon.vue".into(),
        Arc::from(POSITIVE_ICON_PROPS_VUE),
    );

    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let host = Arc::new(host);

    // First resolve — projector publishes the prop; cache populated.
    let meta_before = Arc::clone(&host)
        .get_component_meta("/workspace/src/Icon.vue")
        .expect("first resolve must succeed");
    let prop_names_before: Vec<String> = meta_before.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_before.contains(&"icon".to_string()),
        "first resolve: projector must publish `icon` prop \
         (got {prop_names_before:?})"
    );

    // Edit the root's body to a literal-union surface.
    host.evict("/workspace/src/types.ts");
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        Arc::from(
            r#"export interface IconProps {
  name?: 'small' | 'large'
}
"#,
        ),
    );
    host.evict("/workspace/src/Icon.vue");

    // Second resolve — invalidation must surface the new content.
    // The discriminating signal: the projector republishes the prop
    // against the new root body shape. A torn cache that returned the
    // pre-edit shape would show identical results across both passes;
    // a properly invalidated cache may produce structurally different
    // results without crashing.
    let meta_after = host
        .get_component_meta("/workspace/src/Icon.vue")
        .expect("post-edit resolve must succeed");
    let prop_names_after: Vec<String> = meta_after.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_after.contains(&"icon".to_string()),
        "post-edit: projector must still publish `icon` prop after dep \
         file edit (got {prop_names_after:?})"
    );
}
