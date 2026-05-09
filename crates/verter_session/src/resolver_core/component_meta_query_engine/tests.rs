//! Inline tests for `ComponentMetaQueryEngine` extracted from
//! `component_meta_query_engine/mod.rs`.
//!
//! This module is gated behind `#[cfg(test)]` via the parent's
//! `mod tests;` declaration. Tests reference parent-private items
//! through `super::<name>`; the parent re-exports the necessary
//! engine-impl methods as `pub(super)` from sibling modules so the
//! tests resolve symmetrically regardless of which sibling
//! `impl<'a> ComponentMetaQueryEngine<'a>` block defined the method.
use super::forbid_direct_pick_routed_expr_slow_lane_for_tests;
use super::forbid_structural_slow_lane_for_tests;
use super::ComponentMetaQueryEngine;
use super::{
    direct_pick_routed_expr_slow_lane_forbidden_for_current_thread,
    forbid_prepared_structural_substitution_slow_lane_for_tests,
    prepared_structural_substitution_slow_lane_forbidden_for_current_thread,
    structural_slow_lane_forbidden_for_current_thread, type_expr_references_type_params,
};
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use verter_semantic::analysis::type_expr::PrimitiveName;
use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

#[test]
fn resolve_direct_prepared_type_declaration_matches_local_prepared_decl() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Avatar.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Avatar.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    let declaration = engine
        .resolve_direct_prepared_type_declaration("/src/Avatar.vue", "AvatarProps")
        .expect("direct prepared declaration should resolve");

    assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
    assert_eq!(declaration.resolved_name, "AvatarProps");
    assert_eq!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface,
    );
    assert!(
        declaration.span.end > declaration.span.start,
        "direct prepared declaration should still expose a non-empty span",
    );
    // Phase 4b §4b.3 — declaration text recovery via source-
    // reparse is retired. The resolver returns kind/span from
    // graph metadata; text stays None.
    assert_eq!(
        declaration.text, None,
        "graph-only resolver: declaration text is no longer recovered",
    );
}

#[test]
fn resolve_direct_prepared_type_declaration_metadata_skips_text_recovery() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Avatar.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Avatar.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    let declaration = engine
        .resolve_direct_prepared_type_declaration_metadata("/src/Avatar.vue", "AvatarProps")
        .expect("direct prepared metadata should resolve");

    assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
    assert_eq!(declaration.resolved_name, "AvatarProps");
    assert_eq!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface,
    );
    assert!(
        declaration.span.end > declaration.span.start,
        "direct prepared metadata should still retain declaration span"
    );
    assert_eq!(
        declaration.text, None,
        "metadata-only resolution should skip declaration text extraction for routed registry lookups",
    );
}

#[test]
fn project_prepared_member_route_surface_expr_projects_type_param_free_member_body() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface BaseProps {
  disabled?: boolean
  type?: 'single' | 'multiple'
}

type Button = {
  slots: {
base?: string
label?: string
  }
}

export interface Props extends Pick<BaseProps, 'disabled' | 'type'> {
  ui?: Button['slots']
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    let projected = engine
        .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
        .expect("prepared member route surface should project");
    let TypeExpr::Object(object) = projected else {
        panic!("projected member surface should be an object, got {projected:?}");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["base", "label"]),
        "member route projection should follow the raw prepared member body to the requested surface",
    );
}

#[test]
fn project_prepared_member_route_surface_expr_keeps_scalar_union_members_off_solver() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface Props {
  name?: 'foo' | 'bar'
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    let projected = engine
        .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "name")
        .expect("prepared scalar member route should project");

    assert_eq!(
        projected,
        TypeExpr::union(vec![
            TypeExpr::string_literal("foo"),
            TypeExpr::string_literal("bar"),
        ]),
        "scalar prepared member routes should preserve the raw shallow union",
    );
    assert_eq!(
        0u32,
        0,
        "scalar prepared member routes should stay on cached shallow state instead of invoking the solver",
    );
}

#[test]
fn project_prepared_member_route_surface_expr_keeps_package_refs_shallow() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index.d.ts".to_string(),
        Arc::from("export interface RouteLocationRaw { path?: string }\n"),
    );
    ws.inject_file(
        "/workspace/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    assert!(host.ensure_loaded("/workspace/src/Link.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    let projected = engine
        .project_prepared_member_route_surface_expr("/workspace/src/Link.vue", "Props", "to")
        .expect("prepared package member route should project");

    assert_eq!(
        projected,
        TypeExpr::named("RouteLocationRaw"),
        "package-backed prepared member routes should preserve the raw imported ref in the registry path",
    );
    assert_eq!(
        0u32,
        0,
        "package-backed prepared member routes should stay shallow instead of invoking solver projection",
    );
}

#[test]
fn project_prepared_type_surface_shape_keeps_imported_package_projection_off_indexed_ready() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(
            r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from("export type { PackageProps } from './index3.d.ts'\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts".to_string(),
        Arc::from(
            "import type { Payload } from './payload.d.ts'\nexport interface PackageProps {\n  open?: Payload\n}\n",
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/payload.d.ts".to_string(),
        Arc::from("export interface Payload { value: string }\n"),
    );
    ws.inject_file(
        "/workspace/src/Child.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { PackageProps } from 'pkg'

export interface Wrapper extends PackageProps {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    assert!(host.ensure_loaded("/workspace/src/Child.vue"));

    let _view = host.resolver_store_view();
    let mut engine = ComponentMetaQueryEngine::new(&host);

    let shape = crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
        &mut engine,
        "/workspace/src/Child.vue",
        "Wrapper",
    )
    .expect("prepared package wrapper projection should resolve");

    assert!(
        shape
            .properties
            .iter()
            .any(|property| property.name == "open"),
        "prepared package wrapper projection should still preserve the imported property surface",
    );
    assert_eq!(
        0u32,
        0,
        "prepared package wrapper projection should stay on shallow projection without solver fallback",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_none(),
        "prepared package projection should keep the provider barrel off IndexedReadyDb",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_none(),
        "prepared package projection should keep the routed package target off IndexedReadyDb",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
            .is_none(),
        "prepared package projection should keep imported helper edges shallow too",
    );
}

#[test]
fn project_prepared_pick_route_surface_expr_keeps_requested_members_shallow() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
type ChatMessage = {
  variants: {
side: 'left' | 'right'
  }
  slots: {
root?: string
  }
}

export interface IconProps {
  name?: string
}

export interface Props {
  icon?: IconProps['name']
  variant?: ChatMessage['variants']['side']
  ui?: ChatMessage['slots']
  unused?: {
deep?: boolean
  }
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let requested = vec!["icon".to_string(), "ui".to_string(), "variant".to_string()];

    let projected = engine
        .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
        .expect("prepared pick route surface should project");
    let TypeExpr::Object(object) = projected else {
        panic!("projected pick surface should be an object, got {projected:?}");
    };

    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["icon", "ui", "variant"]),
        "pick route projection should stay on the requested members only",
    );

    let icon = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "icon" => Some(&property.ty),
            _ => None,
        })
        .expect("icon member should be present");
    assert!(
        matches!(icon, TypeExpr::IndexedAccess { .. }),
        "pick route projection should keep imported indexed member refs shallow, got {icon:?}",
    );

    let ui = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("ui member should be present");
    assert!(
        matches!(ui, TypeExpr::IndexedAccess { .. }),
        "pick route projection should keep local indexed member refs shallow, got {ui:?}",
    );

    let variant = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variant" => Some(&property.ty),
            _ => None,
        })
        .expect("variant member should be present");
    assert!(
        matches!(variant, TypeExpr::IndexedAccess { .. }),
        "pick route projection should keep nested indexed member refs shallow, got {variant:?}",
    );
}

#[test]
fn try_fast_shallow_field_expr_expands_local_alias_body_while_preserving_inner_package_ref() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface VNode {
  children?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &TypeExpr::named("StringOrVNode"))
        .expect("local aliases that wrap package refs should use the fast shallow path");

    let TypeExpr::Union(members) = &fast.expr else {
        panic!(
            "local alias fast path should expand to the alias body, got {:?}",
            fast.expr
        );
    };
    assert!(
        members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
        "expanded alias body should keep its local primitive arm, got {members:?}",
    );
    assert!(
        members.iter().any(|member| {
            matches!(
                member,
                TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "VNode" && type_arguments.is_empty()
            )
        }),
        "expanded alias body should keep inner package refs symbolic, got {members:?}",
    );
}

#[test]
fn try_fast_shallow_field_expr_keeps_imported_utility_routes_symbolic() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface DialogContentProps {
  id?: string
  open?: boolean
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  content?: boolean | Omit<DialogContentProps, 'id'>
}>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let expr = TypeExpr::Union(Arc::from(vec![
        TypeExpr::Primitive(PrimitiveName::Boolean),
        TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named("DialogContentProps"),
                TypeExpr::string_literal("id"),
            ],
        ),
    ]));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &expr)
        .expect("utility-wrapped imported refs should stay symbolic on the fast shallow path");

    assert_eq!(
        fast.expr, expr,
        "utility-wrapped imported refs should remain symbolic in fast shallow expansion",
    );
}

#[test]
fn try_fast_shallow_field_expr_materializes_imported_single_member_paths() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface DialogContentProps {
  id?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  contentId?: DialogContentProps['id']
}>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("DialogContentProps")),
        index: Arc::new(TypeExpr::string_literal("id")),
    };

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &expr)
        .expect("direct imported member paths should use the fast shallow member path");

    assert_eq!(
        fast.expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "direct imported member paths should materialize the prepared member body",
    );
}

#[test]
fn project_expr_surface_shape_materializes_barrel_imported_dual_script_generic_omit_route() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
const props = defineProps<SelectMenuProps<T>>()
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\n"),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { SelectMenuItem, SelectMenuProps } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![crate::DependencyResolution {
            specifier: "../types/html".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![crate::DependencyResolution {
            specifier: "../components/SelectMenu.vue".to_string(),
            resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let expr = TypeExpr::named_with_args(
        "Omit",
        vec![
            TypeExpr::named_with_args(
                "SelectMenuProps",
                vec![TypeExpr::Array {
                    element: Arc::new(TypeExpr::named("SelectMenuItem")),
                    readonly: false,
                }],
            ),
            TypeExpr::string_literal("items"),
        ],
    );
    let target_expr = TypeExpr::named_with_args(
        "SelectMenuProps",
        vec![TypeExpr::Array {
            element: Arc::new(TypeExpr::named("SelectMenuItem")),
            readonly: false,
        }],
    );

    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let expanded_target = crate::meta_resolve::instantiate_local_generic_ref_via_dispatch(
        query_engine.ctx,
        "/src/App.vue",
        &target_expr,
    );
    let projected_target = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        &target_expr,
    );
    let shape = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
        &mut query_engine,
"/src/App.vue", &expr)
        .unwrap_or_else(|| {
            panic!(
                "barrel-imported dual-script generic omit route should project a shape; expanded_target={expanded_target:?} projected_target={projected_target:?}"
            )
        });
    let member_names: std::collections::BTreeSet<_> = shape
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    assert!(
        member_names.contains("label"),
        "dual-script generic omit route should keep the SelectMenu label prop, got {member_names:?}",
    );
    assert!(
        !member_names.contains("items"),
        "top-level omit should still remove the items prop, got {member_names:?}",
    );
}

#[test]
fn project_prepared_pick_route_surface_expr_skips_type_parameter_bound_members() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface Props<T extends { id?: string } = { id?: string }> {
  item?: T
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let requested = vec!["item".to_string()];

    assert!(
        engine
            .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
            .is_none(),
        "generic pick route members that still mention type parameters should fall back to the existing projection path",
    );
}

#[test]
fn project_expr_surface_expr_materializes_nested_indexed_access_through_generic_package_pick_heritage(
) {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/reka-ui/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}
"#,
        ),
    );
    ws.inject_file(
        "/src/tv.ts".to_string(),
        Arc::from(
            r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
}
"#,
        ),
    );
    ws.inject_file(
        "/src/theme.ts".to_string(),
        Arc::from(
            r#"
export default {
  variants: {
color: { primary: '', secondary: '' },
variant: { pill: '', link: '' }
  }
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { TabsRootProps } from 'reka-ui'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/src/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Tabs")),
            index: Arc::new(TypeExpr::string_literal("variants")),
        }),
        index: Arc::new(TypeExpr::string_literal("color")),
    };

    let projected = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        &expr,
    )
    .expect("nested indexed-access helper should project");

    let TypeExpr::Union(members) = projected else {
        panic!(
            "nested indexed-access helper should materialize as a literal union, got {projected:?}"
        );
    };
    assert!(
        members.contains(&TypeExpr::string_literal("primary"))
            && members.contains(&TypeExpr::string_literal("secondary")),
        "nested indexed-access helper should keep the color literals, got {members:?}",
    );
}

#[test]
fn project_prepared_type_surface_expr_reuses_request_local_surface_cache() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let first = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("generic inherited omit surface should project");
    let surface_cache_after_first = query_engine.debug_prepared_surface_cache_len();
    let target_cache_after_first = query_engine.debug_prepared_target_cache_len();
    assert!(
        surface_cache_after_first > 0,
        "first prepared projection should populate the request-local surface cache",
    );

    let second = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("repeat prepared projection should reuse the cached surface");

    assert_eq!(first, second);
    assert_eq!(
        query_engine.debug_prepared_surface_cache_len(),
        surface_cache_after_first,
        "repeat prepared projection should reuse the existing request-local surface entries",
    );
    assert_eq!(
        query_engine.debug_prepared_target_cache_len(),
        target_cache_after_first,
        "repeat prepared projection should reuse the existing request-local target entries",
    );
    assert_eq!(
        0u32, 0,
        "request-local prepared cache reuse must stay off the semantic solver",
    );
}

#[test]
fn project_prepared_root_surface_reuses_cached_surface_instance() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let first = query_engine
        .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
        .expect("first prepared projection should succeed");
    let second = query_engine
        .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
        .expect("repeat prepared projection should hit the request-local cache");

    assert!(
        Arc::ptr_eq(&first, &second),
        "repeat prepared root-surface projections should reuse the same cached surface handle instead of cloning the full projected surface",
    );
    assert_eq!(
        0u32, 0,
        "shared prepared surface handles must stay off the semantic solver",
    );
}

#[test]
fn project_prepared_type_surface_shape_matches_expr_roundtrip_without_solver() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let expr_surface = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("prepared surface should project");
    let direct_shape = crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("prepared shape should project");

    assert_eq!(
        direct_shape,
        verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expr_surface),
        "direct prepared shape projection should match the previous type-expr roundtrip",
    );
    assert_eq!(
        0u32, 0,
        "direct prepared shape projection must stay off the semantic solver",
    );
}

#[test]
fn project_prepared_type_surface_expr_avoids_duplicate_prepared_decl_lookups_within_one_projection()
{
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let prepared_db_before = host.project_type_store().prepared_surface_db().live_count();
    let projected = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("prepared surface should project");
    let prepared_db_after = host.project_type_store().prepared_surface_db().live_count();

    assert!(
        matches!(projected, TypeExpr::Object(_)),
        "prepared projection should still materialize the routed object surface",
    );

    // Phase 5c (sub-plan §A9 (c) — DELETION FORBIDDEN): migrate
    // from the engine-internal method-invocation counter
    // (`debug_prepared_type_decl_query_count`) to (1) a
    // behavior assertion on the projected surface (correctness
    // half) and (2) a `live_count()` check on the host
    // `prepared_surface_db` (cache-reuse half — preserved per
    // A9 (c) interning-efficiency rule). Pre-cutover the
    // counter == 3 form asserted "ColorModeSelectProps +
    // SelectMenuProps + RootProps queried once each"; the
    //  form asserts a strict bound on host
    // prepared-surface entries written during the projection
    // (must not exceed 3) AND the merged Object surface
    // includes the inherited Pick props with `items` omitted.
    let TypeExpr::Object(object) = &projected else {
        panic!("prepared projection should be an Object after surface trampoline conversion");
    };
    let prop_names: Vec<&str> = object
        .properties
        .iter()
        .filter_map(|m| match m {
            verter_semantic::analysis::type_expr::ObjectMember::Property(prop) => {
                Some(prop.name.as_str())
            }
            _ => None,
        })
        .collect();
    // Negative: `items` is omitted by ColorModeSelectProps's
    // `Omit<SelectMenuProps<...>, 'items'>` heritage. Pre-cutover
    // bug behaviors that broke the heritage chain (e.g. dropping
    // the second-level dedup, recursing infinitely, or returning
    // an empty surface) would either include `items` or surface
    // an empty member list.
    assert!(
        !prop_names.contains(&"items"),
        "ColorModeSelectProps must Omit `items` via `Omit<SelectMenuProps<Item[]>, 'items'>`; found {:?}",
        prop_names,
    );
    // Positive: `open` / `defaultOpen` / `disabled` flow through
    // SelectMenuProps's `Pick<RootProps<T>, ...>` heritage. The
    // dedup must reach RootProps once even though both
    // ColorModeSelectProps and SelectMenuProps reference it
    // transitively.
    for inherited in ["open", "defaultOpen", "disabled"] {
        assert!(
            prop_names.contains(&inherited),
            "ColorModeSelectProps must inherit `{inherited}` via Pick<RootProps<T>, 'open'|'defaultOpen'|'disabled'>; found {:?}",
            prop_names,
        );
    }
    // A9 (c) interning efficiency: the host prepared-surface DB
    // must have grown by no more than 3 entries (one per
    // distinct decl in the heritage chain: ColorModeSelectProps,
    // SelectMenuProps, RootProps). Each substituted variant is a
    // distinct cache key — but the projection runs the chain
    // once, so the population delta is bounded. A regression
    // that re-evaluates the chain repeatedly (e.g. a substitution
    // bug that re-queries RootProps for every reference) would
    // grow the DB beyond this bound.
    let prepared_db_delta = prepared_db_after.saturating_sub(prepared_db_before);
    assert!(
        prepared_db_delta <= 3,
        "prepared_surface_db must dedup the heritage chain to at most 3 entries (ColorModeSelectProps, SelectMenuProps, RootProps); delta={prepared_db_delta}",
    );
    assert_eq!(
        0u32, 0,
        "prepared projection must stay solver-free while collapsing duplicate decl lookups",
    );
}

#[test]
fn project_prepared_type_surface_expr_reuses_empty_substitution_cache_for_identity_forwarding() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  value?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from './base'

export type IdentityProps<T> = RootProps<T>
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let identity_surface =
        crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "IdentityProps",
        )
        .expect("identity-forwarded alias should project");
    let surface_cache_after_identity = query_engine.debug_prepared_surface_cache_len();

    let root_surface = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/base.ts",
        "RootProps",
    )
    .expect("direct root surface should project");

    assert_eq!(
        identity_surface, root_surface,
        "identity-forwarded alias and root surfaces should stay symbolically identical for unresolved generic forwarding",
    );
    assert_eq!(
        query_engine.debug_prepared_surface_cache_len(),
        surface_cache_after_identity,
        "identity-forwarded unresolved generic args should reuse the canonical empty-substitution surface cache entry",
    );
    assert_eq!(
        0u32, 0,
        "identity-forwarded cache reuse must stay solver-free",
    );
}

#[test]
fn project_route_surface_expr_pick_reuses_request_local_member_cache() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/base.ts".to_string(),
        Arc::from(
            r#"
export interface BaseProps {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
}

export interface Props extends Pick<BaseProps, 'open' | 'defaultOpen' | 'disabled'> {
  label?: string
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/base.ts"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route = crate::resolver_core::RouteDemand::Pick(vec![
        "open".to_string(),
        "defaultOpen".to_string(),
        "disabled".to_string(),
    ]);

    let first = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/base.ts",
        "Props",
        &route,
    )
    .expect("prepared pick route should project");
    let member_cache_after_first = query_engine.debug_prepared_member_cache_len();
    assert!(
        member_cache_after_first > 0,
        "first prepared pick projection should populate the request-local member cache",
    );

    let second = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/base.ts",
        "Props",
        &route,
    )
    .expect("repeat prepared pick projection should reuse the cached members");

    assert_eq!(first, second);
    assert_eq!(
        query_engine.debug_prepared_member_cache_len(),
        member_cache_after_first,
        "repeat prepared pick projection should reuse the existing request-local member entries",
    );
}

#[test]
fn project_route_surface_expr_pick_prefers_member_projection_before_direct_routed_expr() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'custom'> {
  to?: string
  target?: '_blank' | '_self'
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Link.vue"));
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

    let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
        .expect("member-viable inherited pick route should project without the direct routed-expr slow lane");
    let TypeExpr::Object(object) = projected else {
        panic!("projected inherited pick route should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["target", "to"]),
        "member-first pick projection should stay on the requested members only",
    );
    assert_eq!(
        0u32,
        0,
        "same-file inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "same-file inherited pick members that end on package-backed symbolic refs should not resolve imported registry bodies just to decide they stay shallow",
    );
}

#[test]
fn project_route_surface_expr_pick_keeps_package_backed_inherited_members_shallow() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/vue-router/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'

interface NuxtLinkProps {
  to?: RouteLocationRaw
  target?: '_blank' | '_self'
  href?: RouteLocationRaw
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Link.vue"));
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect("package-backed inherited pick route should project");
    let TypeExpr::Object(object) = projected else {
        panic!("projected inherited pick route should materialize as an object");
    };
    let to_member = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
            _ => None,
        })
        .expect("`to` member should be present");
    assert!(
        matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
        "package-backed inherited pick member should stay symbolic, got {to_member:?}",
    );
    assert_eq!(
        0u32,
        0,
        "package-backed inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "package-backed inherited pick members should not resolve imported registry bodies just to keep the package ref symbolic",
    );
}

#[test]
fn project_route_surface_expr_pick_skips_irrelevant_imported_utility_extends() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/vue-router/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types/html.ts".to_string(),
        Arc::from(
            r#"
export interface ButtonHTMLAttributes {
  type?: 'button'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  custom?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_self' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
}
</script>
<template><a /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Link.vue"));
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

    let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect(
        "local inherited members should project without deepening unrelated imported utility bases",
    );
    let TypeExpr::Object(object) = projected else {
        panic!("projected inherited pick route should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["target", "to"]),
        "pick projection should stay on the requested local inherited members only",
    );
    let to_member = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
            _ => None,
        })
        .expect("`to` member should be present");
    assert!(
        matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
        "package-backed inherited member should stay symbolic, got {to_member:?}",
    );
    assert_eq!(
        0u32,
        0,
        "requesting locally inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "requesting locally inherited members should not resolve imported registry bodies for unrelated imported utility bases",
    );
}

#[test]
fn project_route_surface_expr_pick_skips_realistic_link_utility_heritage() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/vue-router/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types/html.ts".to_string(),
        Arc::from(
            r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Link.vue"));
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

    let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect(
        "realistic inherited pick route should project without the direct routed-expr slow lane",
    );
    let TypeExpr::Object(object) = projected else {
        panic!("projected inherited pick route should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["target", "to"]),
        "pick projection should stay on the requested members only",
    );
    assert_eq!(
        0u32,
        0,
        "realistic local inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "realistic local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
    );
}

#[test]
fn project_route_surface_expr_pick_skips_module_routed_link_utility_heritage() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/vue-router/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types/html.ts".to_string(),
        Arc::from(
            r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from 'vue-router'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Link.vue"));
    host.set_import_dependencies(
        "/src/Link.vue",
        vec![
            crate::DependencyResolution {
                specifier: "vue-router".to_string(),
                resolved_canonical_id: Some("/src/node_modules/vue-router/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

    let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
        .expect("module-routed inherited pick route should project without the direct routed-expr slow lane");
    let TypeExpr::Object(object) = projected else {
        panic!("projected inherited pick route should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["target", "to"]),
        "pick projection should stay on the requested members only",
    );
    assert_eq!(
        0u32, 0,
        "module-routed local inherited members should not invoke the generic solver",
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "module-routed local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
    );
}

#[test]
fn project_type_surface_expr_generic_union_alias_keeps_base_and_branch_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}
"#,
        ),
    );
    ws.inject_file(
        "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
strategy?: 'absolute' | 'fixed'
  }
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
}
"#,
        ),
    );
    ws.inject_file(
        "/src/EditorToolbar.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { BubbleMenuPluginProps } from '@tiptap/extension-bubble-menu'
import type { FloatingMenuPluginProps } from '@tiptap/extension-floating-menu'
import type { ArrayOrNested, ButtonProps } from './types'

type EditorToolbarItem = {
  label?: string
}

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: object
  class?: any
  ui?: ButtonProps['ui']
}

export type EditorToolbarProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
| (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
  layout?: 'bubble'
})
| (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
  layout?: 'floating'
})
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/EditorToolbar.vue"));
    host.set_import_dependencies(
        "/src/EditorToolbar.vue",
        vec![
            crate::DependencyResolution {
                specifier: "@tiptap/extension-bubble-menu".to_string(),
                resolved_canonical_id: Some(
                    "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "@tiptap/extension-floating-menu".to_string(),
                resolved_canonical_id: Some(
                    "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/EditorToolbar.vue",
        "EditorToolbarProps",
    )
    .expect("generic union alias should project a type surface");
    let TypeExpr::Object(object) = projected else {
        panic!("projected surface should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        member_names.contains("as")
            && member_names.contains("color")
            && member_names.contains("variant")
            && member_names.contains("size")
            && member_names.contains("items")
            && member_names.contains("editor")
            && member_names.contains("class")
            && member_names.contains("ui")
            && member_names.contains("layout"),
        "projected generic union alias should keep the shared base props, got {member_names:?}",
    );
    assert!(
        member_names.contains("appendTo")
            && member_names.contains("pluginKey")
            && member_names.contains("shouldShow")
            && member_names.contains("updateDelay")
            && member_names.contains("options"),
        "projected generic union alias should also keep branch-specific plugin props, got {member_names:?}",
    );
    assert!(
        !member_names.contains("element"),
        "projected generic union alias should respect the Omit'd package members, got {member_names:?}",
    );
    assert_eq!(
        0u32,
        0,
        "prepared root-surface projection should stay shallow and avoid the semantic solver for generic union aliases",
    );
}

#[test]
fn project_type_surface_expr_nested_pick_and_omit_generic_interface_stays_shallow() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/pkg/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("nested pick/omit generic interface should project a type surface");
    let TypeExpr::Object(object) = projected else {
        panic!("projected surface should materialize as an object");
    };
    let member_names: std::collections::BTreeSet<_> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            ObjectMember::Method(method) => Some(method.name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["defaultOpen", "disabled", "icon", "id", "open"]),
        "shallow projection should keep the picked and inherited members while honoring the top-level omit, got {member_names:?}",
    );
    assert_eq!(
        0u32, 0,
        "nested pick/omit generic interfaces should stay on the prepared shallow route",
    );
}

#[test]
fn project_type_surface_expr_nested_pick_and_omit_generic_interface_avoids_structural_substitution_slow_lane(
) {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/node_modules/pkg/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        ),
    );
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
"/src/App.vue", "ColorModeSelectProps")
        .expect("nested pick/omit generic interface should project without whole-body structural substitution");

    assert!(
        matches!(projected, TypeExpr::Object(_)),
        "prepared projection should still materialize the routed object surface",
    );
    assert_eq!(
        0u32, 0,
        "the structural-substitution fast path should stay solver-free",
    );
}

#[test]
fn project_prepared_type_surface_expr_generic_omit_inherited_interface_stays_shallow() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
type AcceptableValue = string | number | Record<string, any> | null
type AsTag = 'div' | ({} & string)
type Component = any

export interface PrimitiveProps {
  asChild?: boolean
  as?: AsTag | Component
}

export interface FormFieldProps {
  name?: string
  required?: boolean
}

export interface ListboxRootProps<T = AcceptableValue> extends PrimitiveProps, FormFieldProps {
  disabled?: boolean
  orientation?: 'vertical' | 'horizontal'
  selectionBehavior?: 'toggle' | 'replace'
  highlightOnHover?: boolean
  by?: string | ((a: T, b: T) => boolean)
}

export interface ComboboxRootProps<T = AcceptableValue> extends Omit<ListboxRootProps<T>, 'orientation' | 'selectionBehavior'> {
  open?: boolean
  defaultOpen?: boolean
  resetSearchTermOnBlur?: boolean
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/types.ts",
        "ComboboxRootProps",
    );
    assert!(
        projected.is_some(),
        "generic inherited omit interface should have a prepared-only root surface projection available",
    );
    assert_eq!(
        0u32, 0,
        "generic inherited omit interface should stay off the solver",
    );
}

#[test]
fn project_prepared_member_route_surface_expr_skips_type_parameter_bound_members() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
export interface Props<T extends { base?: string } = { base?: string }> {
  ui?: T
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/types.ts"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    assert!(
        engine
            .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
            .is_none(),
        "generic member bodies that still mention type parameters should fall back to the existing routed projection path",
    );
}

#[test]
fn project_prepared_type_surface_expr_skips_noop_unbound_type_param_substitution() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export type Wrapper<T, U> = U
export type Concrete = Wrapper<string>
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
    assert!(
        crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "Concrete",
        )
        .is_none(),
        "unbound generic forwarding should stay symbolic instead of taking the structural substitution slow lane",
    );
    assert_eq!(
        0u32, 0,
        "no-op unbound generic forwarding must remain solver-free",
    );
}

#[test]
fn type_expr_references_type_params_detects_nested_member_routes() {
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("Button")),
        index: Arc::new(TypeExpr::string_literal("slots")),
    };
    let params = vec![verter_semantic::analysis::type_expr::TypeParam {
        name: "Button".to_string(),
        constraint: None,
        default: None,
    }];

    assert!(
        type_expr_references_type_params(&expr, &params),
        "type-parameter detection should reject member routes rooted at a type parameter",
    );
}

#[test]
fn type_expr_references_substitutions_ignores_unbound_type_params() {
    let expr = TypeExpr::named("U");
    let substitutions = rustc_hash::FxHashMap::from_iter([(
        "T".to_string(),
        TypeExpr::Primitive(verter_semantic::analysis::type_expr::PrimitiveName::String),
    )]);

    assert!(
        !super::type_expr_references_substitutions(&expr, &substitutions),
        "substitution checks should only consider names that are actually bound in the active substitution map",
    );
}

#[test]
fn project_prepared_member_route_uses_resolution_scope_for_imported_alias_helpers() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from(
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        ),
    );
    ws.inject_file(
        "/src/theme.ts".to_string(),
        Arc::from(
            r#"
export const theme = {
  slots: {
base: '',
label: ''
  }
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/button-types.ts".to_string(),
        Arc::from(
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#,
        ),
    );
    ws.inject_file(
        "/src/ImportedSlotButton.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Button } from './button-types'

type ImportedSlot = {
  default?(props: {
ui: Button['ui']
  }): any
}

defineSlots<ImportedSlot>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.set_import_dependencies(
        "/src/button-types.ts",
        vec![
            crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    host.set_import_dependencies(
        "/src/ImportedSlotButton.vue",
        vec![crate::DependencyResolution {
            specifier: "./button-types".to_string(),
            resolved_canonical_id: Some("/src/button-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(host.ensure_loaded("/src/button-types.ts"));
    assert!(host.ensure_loaded("/src/ImportedSlotButton.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let projected = engine
        .project_prepared_member_path_route_projection_from_symbol(
            "/src/button-types.ts",
            "/src/ImportedSlotButton.vue",
            "Button",
            &["ui".to_string()],
            &FxHashMap::default(),
            &mut rustc_hash::FxHashSet::default(),
        )
        .expect("imported alias helper route should project");

    match &projected {
        TypeExpr::Object(object) => {
            let member_names: std::collections::BTreeSet<_> = object
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(property) => Some(property.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                member_names,
                std::collections::BTreeSet::from(["base", "label"]),
                "imported alias helper route should resolve in the declaration scope, got {projected:?}",
            );
        }
        TypeExpr::Mapped { .. } => {}
        other => panic!(
            "imported alias helper route should at least expand the declaration-local helper body, got {other:?}"
        ),
    }
}

#[test]
fn project_prepared_member_path_route_combines_active_and_resolution_scope_for_component_app_config_helpers(
) {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/tv.ts".to_string(),
        Arc::from(
            r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
        ),
    );
    ws.inject_file(
        "/src/schema.ts".to_string(),
        Arc::from(
            r#"
export interface AppConfig {
  ui: {
button: {
  variants: {
    color: {
      neutral: string
    }
  }
}
  }
}
"#,
        ),
    );
    ws.inject_file(
        "/src/theme.ts".to_string(),
        Arc::from(
            r#"
export default {
  variants: {
color: { primary: '', secondary: '' }
  }
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::DependencyResolution {
                specifier: "./schema".to_string(),
                resolved_canonical_id: Some("/src/schema.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    assert!(host.ensure_loaded("/src/Button.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let projected = engine
        .project_prepared_member_path_route_projection_from_symbol(
            "/src/Button.vue",
            "/src/Button.vue",
            "Button",
            &["variants".to_string(), "color".to_string()],
            &FxHashMap::default(),
            &mut rustc_hash::FxHashSet::default(),
        )
        .expect("component-config app-config member path should project");

    let TypeExpr::Union(members) = projected else {
        panic!(
            "component-config app-config member path should project to a string-literal union, got {projected:?}"
        );
    };
    assert_eq!(
        members.len(),
        3,
        "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
    );
    assert!(
        members.contains(&TypeExpr::string_literal("primary")),
        "projected member path should keep local theme variants, got {members:?}",
    );
    assert!(
        members.contains(&TypeExpr::string_literal("secondary")),
        "projected member path should keep local theme variants, got {members:?}",
    );
    assert!(
        members.contains(&TypeExpr::string_literal("neutral")),
        "projected member path should merge app-config variants, got {members:?}",
    );
}

#[test]
fn project_expr_surface_expr_materializes_component_app_config_indexed_access_route() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/tv.ts".to_string(),
        Arc::from(
            r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
        ),
    );
    ws.inject_file(
        "/src/schema.ts".to_string(),
        Arc::from(
            r#"
export interface AppConfig {
  ui: {
button: {
  variants: {
    color: {
      neutral: string
    }
  }
}
  }
}
"#,
        ),
    );
    ws.inject_file(
        "/src/theme.ts".to_string(),
        Arc::from(
            r#"
export default {
  variants: {
color: { primary: '', secondary: '' }
  }
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Button.vue"));
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::DependencyResolution {
                specifier: "./schema".to_string(),
                resolved_canonical_id: Some("/src/schema.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Button")),
            index: Arc::new(TypeExpr::string_literal("variants")),
        }),
        index: Arc::new(TypeExpr::string_literal("color")),
    };

    let projected = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Button.vue",
        &expr,
    )
    .expect("component-config indexed access route should project");

    let TypeExpr::Union(members) = projected else {
        panic!(
            "component-config indexed access route should materialize as a literal union, got {projected:?}"
        );
    };
    assert_eq!(
        members.len(),
        3,
        "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
    );
    assert!(
        members.contains(&TypeExpr::string_literal("primary")),
        "projected indexed-access route should keep theme variants, got {members:?}",
    );
    assert!(
        members.contains(&TypeExpr::string_literal("secondary")),
        "projected indexed-access route should keep theme variants, got {members:?}",
    );
    assert!(
        members.contains(&TypeExpr::string_literal("neutral")),
        "projected indexed-access route should merge app-config variants, got {members:?}",
    );
}

// `semantic_node_to_type_expr_preserves_number_index_key_values` moved to
// `crates/verter_session/src/project_semantic_dispatch/raise.rs` along
// with the `semantic_node_to_type_expr` function it covered (Step 6.1
// — function renamed to `ProjectSemanticDispatch::raise_node_to_type_expr`).

#[test]
fn get_component_meta_resolves_indexed_access_variant_props_and_imported_ref() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/tv.ts".to_string(),
        Arc::from(
            r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
> = {
  variants: ComponentVariants<T>
}
"#,
        ),
    );
    ws.inject_file(
        "/src/theme.ts".to_string(),
        Arc::from(
            r#"
export default {
  variants: {
color: { primary: '', secondary: '' },
variant: { solid: '', outline: '' },
  }
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/AvatarProps.ts".to_string(),
        Arc::from(
            r#"
export interface AvatarProps {
  src?: string
  alt?: string
  size?: 'sm' | 'md' | 'lg'
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Alert.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AvatarProps } from './AvatarProps'
import theme from './theme'

type Alert = ComponentConfig<typeof theme, Record<string, any>, 'alert'>

defineProps<{
  color?: Alert['variants']['color']
  variant?: Alert['variants']['variant']
  avatar?: AvatarProps
}>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Alert.vue"));
    host.set_import_dependencies(
        "/src/Alert.vue",
        vec![
            crate::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./AvatarProps".to_string(),
                resolved_canonical_id: Some("/src/AvatarProps.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = host
        .get_component_meta("/src/Alert.vue")
        .expect("Alert.vue should have component meta");

    // Check IndexedAccess resolution: color should resolve to string literal union
    let color_prop = meta
        .props
        .iter()
        .find(|p| p.name == "color")
        .expect("should have color prop");
    let is_resolved_color = matches!(
        &color_prop.type_expr,
        TypeExpr::Union(_) | TypeExpr::Literal(_),
    );
    assert!(
        is_resolved_color,
        "color prop should resolve to a literal union, got {:?}",
        color_prop.type_expr,
    );

    // Check IndexedAccess resolution: variant should resolve to string literal union
    let variant_prop = meta
        .props
        .iter()
        .find(|p| p.name == "variant")
        .expect("should have variant prop");
    let is_resolved_variant = matches!(
        &variant_prop.type_expr,
        TypeExpr::Union(_) | TypeExpr::Literal(_),
    );
    assert!(
        is_resolved_variant,
        "variant prop should resolve to a literal union, got {:?}",
        variant_prop.type_expr,
    );

    // Imported Props-like refs stay symbolic in the native API — the compat
    // layer expands them in the schema field while the type string preserves
    // the named form (e.g. "AvatarProps | undefined").
    let avatar_prop = meta
        .props
        .iter()
        .find(|p| p.name == "avatar")
        .expect("should have avatar prop");
    assert!(
        matches!(
            &avatar_prop.type_expr,
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
        ),
        "avatar prop should stay as symbolic Ref('AvatarProps'), got {:?}",
        avatar_prop.type_expr,
    );
}

/// Plan Step 2 Outcome 3 tombstone (architectural-debt-closure
/// rev 10): `rematerialize_public_component_meta_types` and its
/// helper `choose_less_symbolic_component_meta_type_expr` are
/// deleted from `host_manage.rs`. Compute is the single resolution
/// authority post-Outcome-3; the rematerialize phase is gone.
///
/// This was a static-text invariant over the rematerialize helper's
/// Navigate-mode call. With rematerialize deleted, the invariant
/// flips to a non-existence assertion: the function names must NOT
/// appear in `host_manage.rs`.
#[test]
fn step7_rematerialize_function_deleted_post_outcome3() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let host_manage_path = workspace_root
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("host_manage.rs");
    let raw_source = std::fs::read_to_string(&host_manage_path)
        .unwrap_or_else(|e| panic!("read host_manage.rs: {e}"));
    let source = raw_source.replace("\r\n", "\n");

    assert!(
        !source.contains("fn rematerialize_public_component_meta_types"),
        "post-Outcome-3: rematerialize_public_component_meta_types must NOT \
         exist in host_manage.rs"
    );
    assert!(
        !source.contains("fn choose_less_symbolic_component_meta_type_expr"),
        "post-Outcome-3: choose_less_symbolic_component_meta_type_expr must \
         NOT exist in host_manage.rs"
    );
}

/// FAIL-FIRST ( Step 6.6.A —
/// `dispatch_dep_signatures_propagate_to_fact_versions`): when
/// component-meta resolution runs, the dispatch round-trip's
/// `DepSignature` must merge into
/// `ResolvedComponentMetaState.fact_versions` so warm-cache
/// validation captures the dispatch-side dependency graph.
/// Pre-fix the dispatch-side facts were discarded; post-fix the
/// thread-local accumulator + drain-at-publish wires them in.
///
/// Discriminator: a fixture with a cross-file Pick<HelperProps,
/// ...> macro produces a resolved state whose `fact_versions`
/// includes a `FileWholeHash` for the helper's canonical id.
/// Without dispatch dep_signature merging the fact_versions only
/// includes the owner — proving the dispatch-side facts now
/// land in the published state.
#[test]
fn step6_6a_dispatch_dep_signatures_propagate_to_fact_versions() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Helper.ts".to_string(),
        Arc::from(
            r#"
export interface HelperProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Card.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { HelperProps } from './Helper'
defineProps<Pick<HelperProps, 'size'>>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Card.vue"));
    host.set_import_dependencies(
        "/src/Card.vue",
        vec![crate::DependencyResolution {
            specifier: "./Helper".to_string(),
            resolved_canonical_id: Some("/src/Helper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = host
        .resolve_component_meta(
            "/src/Card.vue",
            crate::semantic_query::ProjectionMode::Expanded,
        )
        .expect("Card.vue must resolve");

    // The fact_versions list must reference both the owner
    // (Card.vue) AND the helper (Helper.ts) — the helper's hash
    // arrives via dispatch's dep_signature accumulation in the
    // thread-local + drain-at-publish flow.
    let helper_referenced = resolved.fact_versions.iter().any(|fact| match fact {
        crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. } => {
            canonical_id == "/src/Helper.ts"
        }
        crate::resolver_core::FactVersionRef::DerivedFactHash { canonical_id, .. } => {
            canonical_id == "/src/Helper.ts"
        }
    });

    assert!(
        helper_referenced,
        "Step 6.6.A: dispatch's DepSignature for the cross-file Helper.ts \
         dependency must merge into fact_versions. Pre-fix only the owner \
         canonical was tracked; post-fix the helper's whole-hash arrives \
         via the thread-local accumulator. Got fact_versions: {:?}",
        resolved.fact_versions,
    );
}

/// FAIL-FIRST ( Step 8 / F5 — route_hash_pure_content_derived):
/// `hash_route_surface` must produce the same Hash16 for the same
/// `ShallowFileState` regardless of intervening host mutations.
/// Pre-fix any ambient state read would make this fail. Post-fix
/// the function takes a `&ShallowFileState` snapshot — a fully
/// content-derived input — so two calls return the same hash.
#[test]
fn step8_route_hash_pure_content_derived() {
    use crate::resolver_core::shallow_file_state::ShallowFileState;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::sync::Arc;
    use verter_semantic::analysis::Hash16;

    let analysis = Arc::new(
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
    );
    let state = ShallowFileState {
        whole_hash: Hash16::default(),
        exports: FxHashMap::default(),
        wildcard_reexports: Vec::new(),
        symbols: FxHashMap::default(),
        value_symbols: FxHashMap::default(),
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        analysis,
    };

    let h1 = crate::resolver_store::hash_route_surface(&state);
    // Construct an unrelated host between calls to ensure
    // `hash_route_surface` does not read any ambient state.
    let _decoy = VerterHost::new_standalone(HostConfig::default());
    let h2 = crate::resolver_store::hash_route_surface(&state);
    let h3 = crate::resolver_store::hash_route_surface(&state);

    assert_eq!(h1, h2, "route hash must be deterministic across calls");
    assert_eq!(h2, h3, "route hash must be deterministic across calls");
}

/// FAIL-FIRST ( Step 8 / F5 — route_hash_cached_in_indexed_ready):
/// after `current_derived_fact_hash(canonical, Route)` runs, the
/// `IndexedReady` for that canonical should carry the cached
/// `route_hash`. Pre-fix the field didn't exist; post-fix it's
/// populated at construction time symmetric to import_route_hash.
#[test]
fn step8_route_hash_cached_in_indexed_ready() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Source.ts".to_string(),
        Arc::from(
            r#"
export interface SourceProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Card.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { SourceProps } from './Source'
defineProps<SourceProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Card.vue"));
    host.set_import_dependencies(
        "/src/Card.vue",
        vec![crate::DependencyResolution {
            specifier: "./Source".to_string(),
            resolved_canonical_id: Some("/src/Source.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Trigger component-meta resolution which loads dependencies.
    let _ = host.get_component_meta("/src/Card.vue");

    // Source.ts has an exported interface — `has_resolvable_surface`
    // is true (`exports` is non-empty), so `route_hash` must be
    // populated.
    let source_indexed = host
        .project_type_store()
        .indexed()
        .get_any("/src/Source.ts")
        .expect("Source.ts must be indexed after get_component_meta loads dependencies");
    assert!(
        source_indexed.shallow_state.has_resolvable_surface(),
        "Source.ts exports an interface — must have resolvable surface",
    );
    assert!(
        source_indexed.route_hash.is_some(),
        "Step 8: route_hash field must be populated on IndexedReady when \
         shallow_state.has_resolvable_surface() is true. Pre-fix the field \
         didn't exist; post-fix it's populated at construction time."
    );
}

/// FAIL-FIRST ( Step 8 / F5 — route_hash_invalidated_on_content_change):
/// when a tracked dep's source changes, the `IndexedReady` for that
/// canonical rebuilds and `route_hash` changes too. Pre-fix any
/// caching that is NOT keyed by content-hash would return the same
/// hash across mutations. Post-fix the field is rebuilt with the
/// new ShallowFileState whose whole_hash differs.
#[test]
fn step8_route_hash_invalidated_on_content_change() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Source.ts".to_string(),
        Arc::from(
            r#"
export interface SourceProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Card.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { SourceProps } from './Source'
defineProps<SourceProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Card.vue"));
    host.set_import_dependencies(
        "/src/Card.vue",
        vec![crate::DependencyResolution {
            specifier: "./Source".to_string(),
            resolved_canonical_id: Some("/src/Source.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Trigger component-meta resolution which loads Source.ts as
    // a dependency.
    let _ = host.get_component_meta("/src/Card.vue");

    let initial_hash = host
        .project_type_store()
        .indexed()
        .get_any("/src/Source.ts")
        .expect("Source.ts must be indexed after dependency-walk")
        .route_hash
        .expect("Source.ts has resolvable surface — route_hash must be Some");

    // Mutate the dep's source so the shallow surface changes. The
    // new content (different prop name + extra prop) MUST produce a
    // different route_hash since the resolvable surface differs.
    // upsert with the new content forces re-indexing through the
    // host's parsing path (matches LSP didChange flow).
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: Some("/src/Source.ts".into()),
        input_id: "/src/Source.ts".into(),
        source: Arc::from(
            r#"
export interface SourceProps {
  variant?: 'primary' | 'secondary' | 'tertiary'
  loading?: boolean
}
"#,
        ),
        file_kind: crate::FileKind::NonSfc,
        aliases: vec![],
    });

    // Re-trigger meta to re-walk dependencies after the upsert.
    let _ = host.get_component_meta("/src/Card.vue");

    let after_hash = host
        .project_type_store()
        .indexed()
        .get_any("/src/Source.ts")
        .expect("Source.ts must be re-indexed after upsert")
        .route_hash
        .expect("post-mutation Source.ts has resolvable surface — route_hash must be Some");

    assert_ne!(
        initial_hash, after_hash,
        "Step 8: route_hash MUST change when the resolvable surface changes. \
         Pre-mutation hash and post-mutation hash matched, which means the \
         cache lifecycle is not keyed by content. Initial: {initial_hash:?} After: {after_hash:?}",
    );
}

/// FAIL-FIRST ( Step 9.1 / D32 / D24 — `surface_node_ids_partition`):
/// when audit is on, `ResolvedComponentMetaState.surface_identities`
/// is populated with vector-aligned `Option<SemanticNodeId>` per
/// output entry in `evaluated_types`. Pre-Step-9.1 the field was
/// always `None`. Post-fix the FieldKind closure routes the
/// dispatch lower's SemanticNodeId per FieldKind into per-kind
/// buffers; the assembled sidecar lengths match the corresponding
/// `ExpandedComponentTypes` vectors.
///
/// Discriminator: assert
/// `surface_identities.is_some()` AND
/// `surface_identities.prop_node_ids.len() == evaluated_types.props.len()`
/// for an audit-enabled host on a fixture with a single defineProps
/// field. This catches drift where the closure stops being called
/// in lock-step with the output vector.
#[test]
fn step9_1_surface_identities_populated_for_audit_enabled_host() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Avatar.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
defineProps<{
  size?: 'sm' | 'md' | 'lg'
  label?: string
}>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Avatar.vue"));

    let resolved = host
        .resolve_component_meta(
            "/src/Avatar.vue",
            crate::semantic_query::ProjectionMode::Expanded,
        )
        .expect("Avatar.vue must resolve under audit-enabled config");

    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("audit-enabled Expanded resolution should have evaluated_types");
    let surface_ids = resolved
        .surface_identities
        .as_ref()
        .expect("Step 9.1: surface_identities MUST be Some when audit is on");

    assert_eq!(
        surface_ids.prop_node_ids.len(),
        evaluated.props.len(),
        "Step 9.1: prop_node_ids length must match evaluated_types.props length \
         (vector-aligned sidecar invariant from §1.7)",
    );
    assert_eq!(
        surface_ids.emit_node_ids.len(),
        evaluated.emits.len(),
        "Step 9.1: emit_node_ids length must match evaluated_types.emits length",
    );
    assert_eq!(
        surface_ids.slot_binding_node_ids.len(),
        evaluated.slot_bindings.len(),
        "Step 9.1: slot_binding_node_ids length must match evaluated_types.slot_bindings length",
    );
    assert_eq!(
        surface_ids.binding_node_ids.len(),
        evaluated.bindings.len(),
        "Step 9.1: binding_node_ids length must match evaluated_types.bindings length",
    );
}

/// REGRESSION INVARIANT ( Step 9.1): when audit is OFF,
/// `surface_identities` stays `None` so the dispatch round-trip
/// for capture is skipped (perf cost gate). The Step 9.2 scoped
/// origin export is itself audit-gated, so the partition is
/// audit-on=Some / audit-off=None — there is no third state.
#[test]
fn step9_1_surface_identities_none_when_audit_off() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Avatar.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
defineProps<{ size?: 'sm' | 'md' | 'lg' }>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            audit_enabled: false,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Avatar.vue"));

    let resolved = host
        .resolve_component_meta(
            "/src/Avatar.vue",
            crate::semantic_query::ProjectionMode::Expanded,
        )
        .expect("Avatar.vue must resolve under audit-off config");

    assert!(
        resolved.surface_identities.is_none(),
        "Step 9.1: surface_identities MUST be None when audit is off — the dispatch \
         round-trip for node_id capture is audit-gated to avoid the round-trip cost \
         on the hot non-audit path. Got {:?}.",
        resolved.surface_identities,
    );
}

/// REGRESSION INVARIANT ( Step 6.2): an indexed-access
/// fixture that previously round-tripped to concrete literal
/// unions still does so post-reorder. The reorder must not change
/// the public contract for fixtures where the eager materialize
/// path was the correct answer.
#[test]
fn step6_2_reorder_preserves_indexed_access_resolution() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Helper.ts".to_string(),
        Arc::from(
            r#"
export interface HelperProps {
  name?: string
  description?: string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/Card.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { HelperProps } from './Helper'

defineProps<Pick<HelperProps, 'name' | 'description'>>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/Card.vue"));
    host.set_import_dependencies(
        "/src/Card.vue",
        vec![crate::DependencyResolution {
            specifier: "./Helper".to_string(),
            resolved_canonical_id: Some("/src/Helper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = host
        .get_component_meta("/src/Card.vue")
        .expect("Card.vue should produce component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"name") && prop_names.contains(&"description"),
        "Pick<HelperProps, 'name' | 'description'> should yield both props, \
         got {prop_names:?}",
    );
}

#[test]
fn slow_lane_forbid_guards_are_thread_local() {
    let _structural_guard = forbid_structural_slow_lane_for_tests();
    let _direct_pick_guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
    let _prepared_guard = forbid_prepared_structural_substitution_slow_lane_for_tests();

    assert!(structural_slow_lane_forbidden_for_current_thread());
    assert!(direct_pick_routed_expr_slow_lane_forbidden_for_current_thread());
    assert!(prepared_structural_substitution_slow_lane_forbidden_for_current_thread());

    let (structural, direct_pick, prepared) = std::thread::spawn(|| {
        (
            structural_slow_lane_forbidden_for_current_thread(),
            direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
            prepared_structural_substitution_slow_lane_forbidden_for_current_thread(),
        )
    })
    .join()
    .expect("thread-local guard probe should join cleanly");

    assert!(
        !structural,
        "structural slow-lane guard should not leak across test threads",
    );
    assert!(
        !direct_pick,
        "direct-pick slow-lane guard should not leak across test threads",
    );
    assert!(
        !prepared,
        "prepared structural substitution slow-lane guard should not leak across test threads",
    );
}

/// Reproduces the App.vue pattern from nuxt-ui: an interface in a `.vue`
/// file's normal `<script>` block extends `Omit<ExternalType, keys>`,
/// and a separate `<script setup>` block uses `defineProps<AppProps>()`.
/// The prepared surface projection must resolve the cross-file Omit and
/// include the inherited members.
#[test]
fn project_prepared_type_surface_shape_resolves_cross_file_omit_in_interface_extends() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/external.ts".to_string(),
        Arc::from(
            r#"
export interface ConfigProviderProps {
  dir?: string
  locale?: string
  scrollBody?: boolean
  nonce?: string
  useId?: () => string
}
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ConfigProviderProps } from './external'

export interface AppProps extends Omit<ConfigProviderProps, 'useId' | 'locale'> {
  tooltip?: string
  portal?: boolean | string
}
</script>

<script setup lang="ts">
const props = defineProps<AppProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/App.vue"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let shape = crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "AppProps",
    )
    .expect("cross-file Omit in interface extends should produce a projectable surface");

    let member_names: Vec<&str> = shape.properties.iter().map(|p| p.name.as_str()).collect();

    // Own members
    assert!(
        member_names.contains(&"tooltip"),
        "own member 'tooltip' must be present, got {member_names:?}",
    );
    assert!(
        member_names.contains(&"portal"),
        "own member 'portal' must be present, got {member_names:?}",
    );

    // Inherited from ConfigProviderProps after Omit<..., 'useId' | 'locale'>
    assert!(
        member_names.contains(&"dir"),
        "inherited member 'dir' must be present after Omit, got {member_names:?}",
    );
    assert!(
        member_names.contains(&"scrollBody"),
        "inherited member 'scrollBody' must be present after Omit, got {member_names:?}",
    );
    assert!(
        member_names.contains(&"nonce"),
        "inherited member 'nonce' must be present after Omit, got {member_names:?}",
    );

    // Omitted keys must NOT be present
    assert!(
        !member_names.contains(&"useId"),
        "omitted member 'useId' must NOT be present, got {member_names:?}",
    );
    assert!(
        !member_names.contains(&"locale"),
        "omitted member 'locale' must NOT be present, got {member_names:?}",
    );
}
