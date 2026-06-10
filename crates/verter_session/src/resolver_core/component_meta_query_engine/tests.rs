//! Inline tests for `ComponentMetaQueryEngine` extracted from
//! `component_meta_query_engine/mod.rs`.
//!
//! This module is gated behind `#[cfg(test)]` via the parent's
//! `mod tests;` declaration. Tests reference parent-private items
//! through `super::<name>`; the parent re-exports the necessary
//! engine-impl methods as `pub(super)` from sibling modules so the
//! tests resolve symmetrically regardless of which sibling
//! `impl<'a> ComponentMetaQueryEngine<'a>` block defined the method.
use super::type_expr_references_type_params;
use super::ComponentMetaQueryEngine;
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use std::sync::Arc;
use verter_type_expr::PrimitiveName;
use verter_type_expr::{ObjectMember, TypeExpr};

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
    // Discriminating invariant: the resolver returns kind/span
    // from graph metadata; declaration text recovery via source
    // reparse is not supported (`text` stays `None`).
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
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ReductionDemand::Published,
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
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
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ReductionDemand::Published,
    )
    .expect("nested indexed-access helper should project");

    let TypeExpr::Union(members) = &projected else {
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect("member-viable inherited pick route should project to the requested members only");
    let TypeExpr::Object(object) = &projected else {
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
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "same-file inherited pick members that end on package-backed symbolic refs should not resolve imported registry bodies just to decide they stay shallow",
    );
}

#[test]
fn project_route_surface_expr_pick_over_class_excludes_non_public_keys() {
    // DISCRIMINATING (fix #5, routed component-meta Pick/Omit helper): the
    // routed Pick/Omit path (`dispatch_routed_expr_surface_expr`) must NOT
    // hand-filter the projected surface by NAME only — it routes through the
    // SHARED builtin Pick/Omit engine, inheriting its public-keyspace gate.
    // `Pick<C, "secret">` over a class whose `secret` is PRIVATE yields an
    // EMPTY surface; `Pick<C, "open">` (public) materialises `open`; an Omit of
    // the public key leaves no non-public member.
    //
    // Discrimination: FAILS on the pre-fix tree where the Pick/Omit arms call
    // `filtered_projected_surface(.., name-only)` over the FULL projected
    // surface (which carries non-public members) — `secret` / `guarded` match
    // the name filter and leak. PASSES once the arms delegate to the shared
    // builtin engine (`dispatch.builtin_type_slot("Pick"/"Omit")` on `[body, keys]`),
    // which public-filters source members before the name predicate.
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/Mixed.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export class MixedClass {
  public open: string = ""
  protected guarded: number = 0
  private secret: boolean = false
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
    assert!(host.ensure_loaded("/src/Mixed.vue"));
    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let member_names = |projected: &TypeExpr| -> std::collections::BTreeSet<String> {
        match projected {
            TypeExpr::Object(object) => object
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(p) => Some(p.name.clone()),
                    ObjectMember::Method(m) => Some(m.name.clone()),
                    _ => None,
                })
                .collect(),
            // A non-object (empty/degenerate) surface carries no members.
            _ => std::collections::BTreeSet::new(),
        }
    };

    // Positive control: Pick of the PUBLIC key materialises it.
    let pub_route = crate::resolver_core::RouteDemand::Pick(vec!["open".to_string()]);
    if let Some(projected) = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Mixed.vue",
        "MixedClass",
        &pub_route,
    ) {
        assert!(
            member_names(&projected).contains("open"),
            "Pick<MixedClass, \"open\"> (public) must materialise `open`: {projected:?}"
        );
    }

    // DISCRIMINATING: Pick of a PRIVATE / PROTECTED key must NOT materialise it.
    for key in ["secret", "guarded"] {
        let route = crate::resolver_core::RouteDemand::Pick(vec![key.to_string()]);
        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/Mixed.vue",
            "MixedClass",
            &route,
        );
        if let Some(projected) = projected {
            assert!(
                !member_names(&projected).contains(key),
                "routed Pick<MixedClass, \"{key}\"> (non-public) must NOT materialise `{key}`: {projected:?}"
            );
        }
    }

    // DISCRIMINATING: Omit of the PUBLIC key must NOT leave the non-public
    // members on the routed surface.
    let omit_route = crate::resolver_core::RouteDemand::Omit(vec!["open".to_string()]);
    if let Some(projected) = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Mixed.vue",
        "MixedClass",
        &omit_route,
    ) {
        let names = member_names(&projected);
        assert!(
            !names.contains("secret") && !names.contains("guarded"),
            "routed Omit<MixedClass, \"open\"> must NOT leave non-public members: {projected:?}"
        );
    }
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
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
    let TypeExpr::Object(object) = &projected else {
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect(
        "local inherited members should project without deepening unrelated imported utility bases",
    );
    let TypeExpr::Object(object) = &projected else {
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect("realistic inherited pick route should project to the requested members only");
    let TypeExpr::Object(object) = &projected else {
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);
    let route =
        crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

    let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/Link.vue",
        "LinkProps",
        &route,
    )
    .expect("module-routed inherited pick route should project to the requested members only");
    let TypeExpr::Object(object) = &projected else {
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/EditorToolbar.vue",
        "EditorToolbarProps",
    )
    .expect("generic union alias should project a type surface");
    let TypeExpr::Object(object) = &projected else {
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("nested pick/omit generic interface should project a type surface");
    let TypeExpr::Object(object) = &projected else {
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
}

#[test]
fn project_type_surface_expr_nested_pick_and_omit_generic_interface_keeps_exact_shallow_surface() {
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect("nested pick/omit generic interface should project the routed object surface");

    let TypeExpr::Object(object) = &projected else {
        panic!("prepared projection should still materialize the routed object surface");
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
    // The routed surface must reproduce the EXACT shallow member surface
    // the standard route produces for this fixture (covered by
    // `..nested_pick_and_omit_generic_interface_stays_shallow`):
    // `Omit<SelectMenuProps<Item[]>, 'items'>` over the picked
    // `RootProps` members + `IconProps` + the `Omit`'d `HtmlAttrs`. A
    // regression that mangles the surface (drops an inherited member,
    // leaks the `items`/`type`/`disabled`/`name` Omit'd members, or
    // fails to honor the pick) flips this RED.
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["defaultOpen", "disabled", "icon", "id", "open"]),
        "routed surface should keep the picked + inherited members while \
         honoring the nested omits, got {member_names:?}",
    );
}

/// Route-key resolution for `keyof ComponentConfig<typeof theme>['variants']['color']`
/// — the generic-`Ref` indexed-access key source consumed by `Pick`/`Omit`
/// keys (`route_keys.rs` `enumerate_member_surface_keys_via_route`, the
/// `IndexedAccess { object: Ref<Args>, .. }` arm).
///
/// Dispatch-coverage assertion (the prepared-substitution slow lane and
/// its forbid guard have been DELETED): the dispatch-backed leaf
/// stabiliser (`solve_or_project_prepared_member_leaf_expr`, called at
/// the top of `enumerate_member_surface_keys_via_route`) must resolve the
/// generic config's `['variants']['color']` surface to the concrete
/// `{ primary; secondary }` object so the literal keys enumerate via the
/// dispatch fast lane alone. The positive key assertion flips RED if the
/// dispatch leaf stabiliser regresses on this generic-`Ref`
/// indexed-access route.
#[test]
fn enumerate_route_literal_keys_generic_ref_indexed_access_stays_off_substitution_slow_lane() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    // Generic config whose indexed `['variants']['color']` surface has
    // CONCRETE keys (`primary`/`secondary`) regardless of the type
    // argument `T` — so dispatch can stabilise the surface without
    // instantiating the generic body. `T` only types the member VALUES.
    ws.inject_file(
        "/src/cfg.ts".to_string(),
        Arc::from(
            r#"
export interface ComponentConfig<T> {
  variants: {
    color: {
      primary: T
      secondary: T
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
  spacing: 4
} as const
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComponentConfig } from './cfg'
import theme from './theme'
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
                specifier: "./cfg".to_string(),
                resolved_canonical_id: Some("/src/cfg.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `keyof ComponentConfig<typeof theme>['variants']['color']`
    let key_source = TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named_with_args(
                "ComponentConfig",
                vec![TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path: vec!["theme".to_string()],
                })],
            )),
            index: Arc::new(TypeExpr::string_literal("variants")),
        }),
        index: Arc::new(TypeExpr::string_literal("color")),
    }));

    let keys = query_engine
        .enumerate_route_literal_keys("/src/App.vue", "/src/App.vue", &key_source)
        .expect(
            "keyof ComponentConfig<typeof theme>['variants']['color'] should enumerate its literal \
             keys via the dispatch fast lane without the structural-substitution slow lane",
        );
    let key_set: std::collections::BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    assert_eq!(
        key_set,
        std::collections::BTreeSet::from(["primary", "secondary"]),
        "generic-Ref indexed-access key source should yield exactly the concrete color keys, \
         got {key_set:?}",
    );
}

/// Direct utility-shape projection for `Partial<Wrapper<number>>` — exercises
/// the `route_keys.rs` `project_direct_utility_surface_shape` →
/// `projected_target_shape` path. The prepared-substitution engine
/// fallback that once lived at the tail of `projected_target_shape` has
/// been DELETED; this is now a pure dispatch-coverage assertion.
///
/// `Partial<T>` (single type argument) is NOT recognised by
/// `component_meta_registry_public_utility_route` (which matches only 2-arg
/// `Pick`/`Omit`), so `project_expr_surface_shape_via_host_threaded` skips the
/// registry route and enters `project_direct_utility_surface_shape`, which
/// projects the `Wrapper<number>` target shape.
///
/// Discriminator: `projected_target_shape`'s dispatch arms
/// (`project_expr_surface_shape_via_host_threaded` /
/// `project_expr_surface_expr_via_host_threaded`) must resolve the concrete
/// `{ value; label }` surface via dispatch alone. The positive
/// `{ value; label }` + all-optional assertions flip RED on a mangled
/// surface.
#[test]
fn project_direct_utility_partial_generic_stays_off_substitution_slow_lane() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
type Wrapper<T> = { value: T; label: string }
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `Partial<Wrapper<number>>` — utility wrapper over an instantiated
    // generic alias whose body is a structural object.
    let expr = TypeExpr::named_with_args(
        "Partial",
        vec![TypeExpr::named_with_args(
            "Wrapper",
            vec![TypeExpr::Primitive(PrimitiveName::Number)],
        )],
    );

    let shape = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        &expr,
    )
    .expect(
        "Partial<Wrapper<number>> should project its target shape via the dispatch fast lane \
         without the structural-substitution slow lane",
    );
    let member_names: std::collections::BTreeSet<&str> = shape
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["label", "value"]),
        "Partial<Wrapper<T>> should keep both structural members, got {member_names:?}",
    );
    // `Partial` makes every member optional — a positive shape assertion that
    // also confirms the utility wrapper was actually applied (not bypassed).
    assert!(
        shape.properties.iter().all(|property| property.optional),
        "Partial<...> must mark every member optional",
    );
    // Substitution coverage: dispatch's core guarantee is that it
    // SUBSTITUTES the generic arg (it is the sole substitution path —
    // there is no `substitute_type_expr`). Assert the concrete member TYPES, not just
    // names: a regression returning `{ value: T; label: string }` (no
    // substitution) would pass the name/optional checks above but flip these.
    let value_ty = shape
        .properties
        .iter()
        .find(|property| property.name.as_str() == "value")
        .map(|property| &property.ty);
    assert!(
        matches!(value_ty, Some(TypeExpr::Primitive(PrimitiveName::Number))),
        "dispatch must substitute Wrapper's `T` -> number in `value`, got {value_ty:?}",
    );
    let label_ty = shape
        .properties
        .iter()
        .find(|property| property.name.as_str() == "label")
        .map(|property| &property.ty);
    assert!(
        matches!(label_ty, Some(TypeExpr::Primitive(PrimitiveName::String))),
        "`label` must stay `string`, got {label_ty:?}",
    );
}

/// Function generic shadowing through the dispatch instantiation path
/// (`instantiate_local_generic_ref_via_dispatch`), the route that
/// survives the prepared-substitution slow-lane deletion.
///
/// Legacy `substitute_function_expr` (surface.rs) removed the function's
/// OWN type parameters from the substitution map before substituting the
/// params/return, so an inner `<T>` that SHADOWS the outer generic `T`
/// was preserved (not replaced by the outer argument). Dispatch's
/// function-lowering (`lower.rs` `TypeExpr::Function`) is a possible
/// gap because it lowers params/return with the OUTER `env`
/// unchanged.
///
/// Fixture: `type F<T> = <T>(x: T) => T` instantiated as `F<string>`.
/// The instantiation binds the OUTER `T -> string`, but the function
/// body's own `<T>` must shadow that binding — so the parameter type and
/// return type must REMAIN the inner type parameter `T`, NOT collapse to
/// `string`.
///
/// Discriminator: if function-lower used the outer env unchanged the
/// param/return would lower to `Primitive(String)` (the negative
/// assertions below would flip RED); shadowing keeps them as the inner
/// type parameter named `T`.
#[test]
fn instantiate_generic_function_alias_preserves_shadowing_inner_type_param_via_dispatch() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
type F<T> = <T>(x: T) => T
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let query_engine = ComponentMetaQueryEngine::new(&host);

    // `F<string>` — instantiate the generic function-typed alias with the
    // outer `T` bound to `string`.
    let expr = TypeExpr::named_with_args("F", vec![TypeExpr::Primitive(PrimitiveName::String)]);

    let instantiated = crate::meta_resolve::instantiate_local_generic_ref_via_dispatch(
        query_engine.ctx,
        "/src/App.vue",
        &expr,
    )
    .expect(
        "F<string> over a function-typed generic alias should instantiate to the function body \
         via the dispatch instantiation path",
    );

    // The instantiated body is the function type. Its OWN `<T>` shadows
    // the outer `T`, so neither the parameter type nor the return type
    // may be substituted to the outer `string` argument.
    let TypeExpr::Function(function) = &instantiated else {
        panic!("F<string> should instantiate to a Function body, got {instantiated:?}");
    };

    let param_ty = &function
        .parameters
        .first()
        .expect("the shadowing function takes one parameter `x`")
        .ty;
    let return_ty = function
        .return_type
        .as_deref()
        .expect("the shadowing function declares a return type");

    // Helper: does this expr resolve to the inner type parameter named `T`?
    fn is_inner_type_param_t(expr: &TypeExpr) -> bool {
        match expr {
            TypeExpr::TypeParameter(param) => param.name == "T",
            // Top-level alias bodies keep bare `T` as a zero-arg Ref;
            // accept either spelling so the test is robust to the
            // function-param normalisation pass.
            TypeExpr::Ref {
                name,
                type_arguments,
            } => type_arguments.is_empty() && name.as_ref() == "T",
            _ => false,
        }
    }

    // NEGATIVE: the outer `string` argument must NOT have leaked into the
    // shadowed function body. (This is the assertion that flips RED if
    // function-lower substitutes with the outer env unchanged.)
    assert!(
        !matches!(param_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "inner <T> shadows the outer T: parameter type must NOT be substituted to the outer \
         string argument, got {param_ty:?}",
    );
    assert!(
        !matches!(return_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "inner <T> shadows the outer T: return type must NOT be substituted to the outer \
         string argument, got {return_ty:?}",
    );

    // POSITIVE: the param/return must REMAIN the inner type parameter `T`.
    assert!(
        is_inner_type_param_t(param_ty),
        "inner <T> shadows the outer T: parameter type must remain the inner type parameter T, \
         got {param_ty:?}",
    );
    assert!(
        is_inner_type_param_t(return_ty),
        "inner <T> shadows the outer T: return type must remain the inner type parameter T, \
         got {return_ty:?}",
    );
}

/// Dispatch-coverage for the (now-deleted) route_keys.rs:599 utility
/// route — `project_direct_utility_surface_shape` → `projected_target_shape`.
/// NON-structural generic alias body: a `Conditional`.
///
/// The structural-body coverage at this route is
/// `project_direct_utility_partial_generic_stays_off_substitution_slow_lane`
/// (`Partial<Wrapper<number>>`, Object body). This test locks the
/// non-structural case: dispatch's `build_instantiate`
/// `_ => shallow_lower_type_expr_with_context` catch-all (build.rs) must
/// resolve a `Conditional` body correctly via the dispatch surface
/// bridges alone (the prepared-substitution engine fallback at this
/// route has been deleted).
///
/// Fixture: `type Cond<T> = T extends number ? { value: T; label: string } : never`
/// projected as `Partial<Cond<number>>`. `Cond`'s body is a `Conditional`
/// (non-structural). Dispatch must evaluate the conditional to the
/// `{ value; label }` object so `Partial` can mark both members optional.
///
/// Discriminator: the positive `{ value; label }` + all-optional
/// assertions flip RED if dispatch's instantiation catch-all regresses
/// for conditionals.
#[test]
fn project_direct_utility_conditional_body_generic_stays_off_substitution_slow_lane() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
type Cond<T> = T extends number ? { value: T; label: string } : never
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `Partial<Cond<number>>` — utility wrapper over an instantiated
    // generic alias whose body is a NON-structural `Conditional`.
    let expr = TypeExpr::named_with_args(
        "Partial",
        vec![TypeExpr::named_with_args(
            "Cond",
            vec![TypeExpr::Primitive(PrimitiveName::Number)],
        )],
    );

    let shape = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        &expr,
    )
    .expect(
        "Partial<Cond<number>> should project its conditional-resolved target shape via the \
         dispatch fast lane without the structural-substitution slow lane",
    );
    let member_names: std::collections::BTreeSet<&str> = shape
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["label", "value"]),
        "Partial<Cond<number>> should resolve the conditional's number branch to both members, \
         got {member_names:?}",
    );
    assert!(
        shape.properties.iter().all(|property| property.optional),
        "Partial<...> must mark every member of the conditional-resolved surface optional",
    );
    // Substitution coverage: assert dispatch evaluated the conditional's true
    // branch AND substituted `T` -> number — not merely that a `{ value; label }`
    // surface exists. A regression yielding `{ value: T; label: string }` would
    // pass the name/optional checks above but flip these.
    let value_ty = shape
        .properties
        .iter()
        .find(|property| property.name.as_str() == "value")
        .map(|property| &property.ty);
    assert!(
        matches!(value_ty, Some(TypeExpr::Primitive(PrimitiveName::Number))),
        "dispatch must resolve Cond<number>'s true branch with `T` -> number in `value`, got {value_ty:?}",
    );
    let label_ty = shape
        .properties
        .iter()
        .find(|property| property.name.as_str() == "label")
        .map(|property| &property.ty);
    assert!(
        matches!(label_ty, Some(TypeExpr::Primitive(PrimitiveName::String))),
        "`label` must stay `string`, got {label_ty:?}",
    );
}

/// Dispatch-coverage for the (now-deleted) route_keys.rs:465 generic-Ref
/// indexed-access arm of `enumerate_member_surface_keys_via_route`.
/// NON-structural generic alias body: a `Conditional`. Generic-Ref
/// instantiation in that arm now goes through
/// `instantiate_local_generic_ref_via_dispatch` (the prepared-substitution
/// engine fallback that once routed it has been deleted).
///
/// The structural-body coverage at this route is
/// `enumerate_route_literal_keys_generic_ref_indexed_access_stays_off_substitution_slow_lane`
/// (`ComponentConfig<T>` with a structural Object body). This locks the
/// non-structural case: a generic alias whose WHOLE body is a
/// `Conditional` resolving to the nested indexed object.
///
/// Fixture: `type Cfg<T> = T extends true ? { variants: { color: { primary: 1; secondary: 2 } } } : never`
/// with key source `keyof Cfg<true>['variants']['color']`. The body is a
/// `Conditional` (non-structural); dispatch must resolve
/// `Cfg<true>['variants']['color']` to the concrete `{ primary; secondary }`
/// object so the literal keys enumerate via the dispatch fast lane.
///
/// Discriminator: the positive `{ primary, secondary }` key assertion
/// flips RED if dispatch regresses on the conditional-bodied
/// indexed-access route.
#[test]
fn enumerate_route_literal_keys_conditional_body_indexed_access_stays_off_substitution_slow_lane() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/cfg.ts".to_string(),
        Arc::from(
            r#"
export type Cfg<T> = T extends true
  ? { variants: { color: { primary: 1; secondary: 2 } } }
  : never
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { Cfg } from './cfg'
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
        vec![crate::DependencyResolution {
            specifier: "./cfg".to_string(),
            resolved_canonical_id: Some("/src/cfg.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `keyof Cfg<true>['variants']['color']`
    let key_source = TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named_with_args(
                "Cfg",
                vec![TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(
                    true,
                ))],
            )),
            index: Arc::new(TypeExpr::string_literal("variants")),
        }),
        index: Arc::new(TypeExpr::string_literal("color")),
    }));

    let keys = query_engine
        .enumerate_route_literal_keys("/src/App.vue", "/src/App.vue", &key_source)
        .expect(
            "keyof Cfg<true>['variants']['color'] over a conditional-bodied generic alias should \
             enumerate its literal keys via the dispatch fast lane without the \
             structural-substitution slow lane",
        );
    let key_set: std::collections::BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    assert_eq!(
        key_set,
        std::collections::BTreeSet::from(["primary", "secondary"]),
        "conditional-bodied generic indexed-access key source should yield exactly the concrete \
         color keys, got {key_set:?}",
    );
}

/// Dispatch-coverage for the (now-deleted) route_keys.rs:215
/// `Unknown`-fallback arm of `enumerate_member_surface_keys_via_route`.
/// NON-structural generic alias body: a `Ref` to another generic alias.
///
/// That arm previously instantiated the generic alias via the
/// prepared-substitution engine fallback when the dispatch leaf
/// stabiliser returned `Unknown`; the fallback has been DELETED (an
/// `Unknown` leaf is now a clean miss). This locks the non-structural
/// case where the alias body is itself a generic `Ref` (alias-to-alias)
/// rather than an inline structural shape — dispatch must resolve it
/// directly.
///
/// Fixture: `type Inner<T> = { variants: { color: { primary: T; secondary: T } } }`
/// and `type Outer<T> = Inner<T>` (Outer's body is a NON-structural
/// `Ref`). Key source `keyof Outer<number>['variants']['color']`.
/// Dispatch must chase the alias-to-alias indirection and resolve the
/// nested indexed object so the keys enumerate via the dispatch fast
/// lane.
///
/// Discriminator: the positive `{ primary, secondary }` key assertion
/// flips RED if dispatch leaves the route `object` unresolved on the
/// alias-to-alias indirection.
#[test]
fn enumerate_route_literal_keys_alias_to_alias_body_stays_off_substitution_slow_lane() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/cfg.ts".to_string(),
        Arc::from(
            r#"
export type Inner<T> = { variants: { color: { primary: T; secondary: T } } }
export type Outer<T> = Inner<T>
"#,
        ),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { Outer } from './cfg'
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
        vec![crate::DependencyResolution {
            specifier: "./cfg".to_string(),
            resolved_canonical_id: Some("/src/cfg.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `keyof Outer<number>['variants']['color']`
    let key_source = TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named_with_args(
                "Outer",
                vec![TypeExpr::Primitive(PrimitiveName::Number)],
            )),
            index: Arc::new(TypeExpr::string_literal("variants")),
        }),
        index: Arc::new(TypeExpr::string_literal("color")),
    }));

    let keys = query_engine
        .enumerate_route_literal_keys("/src/App.vue", "/src/App.vue", &key_source)
        .expect(
            "keyof Outer<number>['variants']['color'] over an alias-to-alias generic body should \
             enumerate its literal keys via the dispatch fast lane without the \
             structural-substitution slow lane",
        );
    let key_set: std::collections::BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    assert_eq!(
        key_set,
        std::collections::BTreeSet::from(["primary", "secondary"]),
        "alias-to-alias generic indexed-access key source should yield exactly the concrete \
         color keys, got {key_set:?}",
    );
}

#[test]
fn type_expr_references_type_params_detects_nested_member_routes() {
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("Button")),
        index: Arc::new(TypeExpr::string_literal("slots")),
    };
    let params = vec![verter_type_expr::TypeParam {
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
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
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ProjectionMode::Expanded,
        crate::semantic_query::ReductionDemand::Published,
    )
    .expect("component-config indexed access route should project");

    let TypeExpr::Union(members) = &projected else {
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

/// Tombstone guard: `rematerialize_public_component_meta_types` and
/// its helper `choose_less_symbolic_component_meta_type_expr` must
/// NOT exist in `host_manage.rs`. Compute is the single resolution
/// authority; the rematerialize helper family is not part of the
/// final design and re-introducing it would regress dispatch
/// behaviour. The invariant is a non-existence assertion over the
/// production source text.
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
        // R26 per-domain variants — not emitted on this code
        // path today; producers that populate them are not yet
        // wired here. This test characterises the whole-hash flow
        // and asserts the helper canonical participates regardless
        // of which `FactVersionRef` variant carries it.
        crate::resolver_core::FactVersionRef::Parse(p) => p.canonical_id == "/src/Helper.ts",
        crate::resolver_core::FactVersionRef::ResolveImports(r) => {
            r.canonical_id == "/src/Helper.ts"
        }
        crate::resolver_core::FactVersionRef::RouteSurface(r) => r.canonical_id == "/src/Helper.ts",
        // Not file-scoped — references no canonical.
        crate::resolver_core::FactVersionRef::ProjectGeneration { .. } => false,
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
        augmentation_scopes: FxHashMap::default(),
        augmentation_value_scopes: FxHashMap::default(),
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
        file_language: crate::FileLanguage::script_ts(),
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

/// Workspace classification must consult the typed
/// `ResolverContext::workspace_is_package_backed` accessor — NOT
/// `path.contains("/node_modules/")`.
///
/// Discriminator: a workspace package whose root happens to live
/// inside `/node_modules/` (legal under pnpm workspace layouts).
/// Files under that root have:
///   - `canonical_id.contains("/node_modules/")` == TRUE
///     (a substring-based body would misclassify as package-backed)
///   - `ctx.workspace_is_package_backed(canonical_id)` == FALSE
///     (the project root claims the file → workspace-owned)
///
/// This test calls the migrated helpers (`is_package_source`,
/// `is_package_canonical`) directly and asserts they return
/// `false` for the workspace-linked-package canonical, matching
/// the typed accessor's classification. A substring-based body
/// would return `true` here and fail the assertion.
#[test]
fn workspace_classification_helpers_use_typed_accessor_not_substring() {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![verter_workspace::VfsProjectConfig {
            // Project root that itself lives inside `node_modules/` —
            // a workspace-linked package. The workspace classifier
            // claims files under this root as workspace-owned
            // because the suffix between root and file contains no
            // further `/node_modules/` segment.
            root: "/workspace/node_modules/@me/inner-pkg".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/workspace/node_modules/@me/inner-pkg/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/workspace/node_modules/@me/inner-pkg".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ProjectMembership::MatchAll,
        }]);
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(project_graph);
    let workspace_linked_canonical = "/workspace/node_modules/@me/inner-pkg/src/types.ts";
    workspace.inject_file(
        workspace_linked_canonical.into(),
        Arc::from("export interface Foo { value: string }"),
    );
    let ws_access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace/node_modules/@me/inner-pkg".to_string(),
            "/workspace/node_modules/@me/inner-pkg".to_string(),
            Some("/workspace/node_modules/@me/inner-pkg/tsconfig.json".to_string()),
        ),
    ]);
    assert!(host.ensure_loaded(workspace_linked_canonical));

    // Sanity: the typed accessor on the host's resolver-context
    // surface must classify the workspace-linked-package canonical
    // as workspace-owned (NOT package-backed). The substring check
    // on the same canonical would return `true` (path contains
    // `/node_modules/`).
    let ctx: &dyn super::super::ResolverContext = &host;
    assert!(
        ctx.workspace_is_workspace_owned(workspace_linked_canonical),
        "workspace-linked package must be workspace-owned per typed accessor",
    );
    assert!(
        !ctx.workspace_is_package_backed(workspace_linked_canonical),
        "workspace-linked package must NOT be package-backed per typed accessor",
    );
    assert!(
        workspace_linked_canonical.contains("/node_modules/"),
        "fixture sanity: canonical_id must contain `/node_modules/` so the \
         substring check would have returned true (the bug the migration fixes)",
    );

    // Exercise the migrated helpers directly. Each must return
    // `false` (matching the typed accessor), NOT `true` — a
    // substring-based body would return `true` here (the canonical
    // contains `/node_modules/`), failing the assertion.
    assert!(
        !super::helpers::is_package_canonical(ctx, workspace_linked_canonical),
        "is_package_canonical must consult ctx.workspace_is_package_backed; a \
         substring body returns `true` for workspace-linked packages and fails this assertion",
    );
    assert!(
        !super::helpers::is_package_source(ctx, Some(workspace_linked_canonical)),
        "is_package_source must consult ctx.workspace_is_package_backed; a \
         substring body returns `true` for workspace-linked packages and fails this assertion",
    );
}

/// Discriminating regression for the imported-registry recompute bug.
///
/// `resolve_imported_registry_symbol`'s producer used to handle a
/// `None`-refuse-admission outcome by RE-RUNNING
/// `resolve_imported_registry_symbol_with_budget` in a fallback arm.
/// That second run consumed the wildcard-route fuse
/// (`allow_wildcard_route()` / `wildcard_route_fanout`) a SECOND time —
/// so a request near the fanout limit would trip the fuse on the
/// recompute and spuriously resolve a slow-lane imported symbol to
/// `None`, even though the first (uncached) compute had found a value.
///
/// The fixture pins all three discriminators of the bug:
///
///  - `ButtonProps` is a re-export (`export { Props as ButtonProps }
///    from './types'`) — it has NO local prepared declaration in
///    `index.ts`, so `resolve_imported_registry_symbol_with_budget`
///    takes the slow lane and consumes one `allow_wildcard_route()`
///    tick per invocation.
///  - The wildcard-route fuse is primed so EXACTLY ONE further
///    slow-lane resolution stays within budget; a second trips it.
///  - Shared-cache admission is forced to be refused, so the producer
///    must reuse the freshly-computed value on the refused path.
///
/// Discrimination property — FAILS pre-fix, PASSES post-fix:
///
///  - Pre-fix: the producer resolves once inside the `get_or_compute`
///    closure (fuse → 1), `get_or_compute` returns `None` (admission
///    refused), and the fallback arm resolves AGAIN (fuse → 2 > budget
///    → `wildcard_route_fanout` trips → `None`). The resolver runs
///    twice and the request returns `None`.
///  - Post-fix: the producer resolves EXACTLY ONCE outside the closure
///    (fuse → 1), the closure only builds the signature, and the
///    refused-admission path returns the already-computed value. The
///    resolver runs once and the request returns the resolved symbol.
#[test]
fn resolve_imported_registry_symbol_reuses_value_on_admission_failure_without_refusing_fuse() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from("export interface Props { primary: string }"),
    );
    ws.inject_file(
        "/src/index.ts".to_string(),
        Arc::from("export { Props as ButtonProps } from './types'"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/index.ts"));
    assert!(host.ensure_loaded("/src/types.ts"));
    // Wire the barrel's `./types` re-export so the slow-lane
    // `resolve_named_type_export_target_shallow` route reaches the
    // defining file.
    host.set_import_dependencies(
        "/src/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut engine = ComponentMetaQueryEngine::new(&host);
    // Prime the wildcard-route fuse so exactly ONE further slow-lane
    // `allow_wildcard_route()` stays within budget — a second
    // resolution tips it past `wildcard_route_fanout` and trips. This
    // is the near-fanout boundary the recompute bug spuriously fails
    // at.
    engine.prime_wildcard_route_fuse_for_tests(1);
    let fuse_before = engine.wildcard_route_fuse_consumed_for_tests();

    // Force shared-cache admission to be refused for this request, so
    // the producer's refused-admission path is exercised.
    let _refusal = super::force_imported_registry_admission_refusal_for_tests();
    super::reset_imported_registry_resolve_invocations_for_tests();

    let resolved = engine.resolve_imported_registry_symbol("/src/index.ts", "ButtonProps");

    let resolve_invocations = super::imported_registry_resolve_invocations_for_tests();
    let fuse_consumed = engine
        .wildcard_route_fuse_consumed_for_tests()
        .saturating_sub(fuse_before);

    assert_eq!(
        resolve_invocations, 1,
        "resolve_imported_registry_symbol MUST resolve the imported symbol exactly ONCE \
         even when shared-cache admission is refused — the pre-fix producer re-ran \
         resolve_imported_registry_symbol_with_budget in the None-admission fallback, \
         resolving twice",
    );
    assert_eq!(
        fuse_consumed, 1,
        "the wildcard-route fuse MUST be consumed exactly ONCE — the pre-fix recompute \
         consumed it a second time, which near wildcard_route_fanout trips the fuse and \
         spuriously resolves the imported symbol to None",
    );
    assert!(
        !engine.has_fuse_tripped(),
        "no fuse may trip — the single permitted slow-lane resolution stays within \
         budget; the pre-fix second resolution trips wildcard_route_fanout",
    );
    let resolved = resolved
        .expect("the slow-lane imported symbol must still resolve on the refused-admission path");
    assert_eq!(
        (
            resolved.canonical_id.as_str(),
            resolved.exported_name.as_str()
        ),
        ("/src/types.ts", "Props"),
        "the refused-admission path must return the freshly-computed resolved symbol \
         (the defining export), not a recompute and not None",
    );
}

/// Discriminating regression for the discarded-concurrent-result bug.
///
/// `resolve_imported_registry_symbol`'s producer routes a cold miss
/// through `ImportedRegistryDb::get_or_compute`, used purely as a
/// signature-building write-through. `get_or_compute` returns
/// `Option<Option<Arc<_>>>`:
///
///  - `Some(cached)` — a validated value is authoritative: either this
///    request's freshly-admitted compute, OR an entry a CONCURRENT
///    request validated-and-published into the DB between this
///    request's `peek` miss and its `get_or_compute` call (the warm-hit
///    `validate` arm returns it WITHOUT running the closure).
///  - `None` — shared-cache admission was refused.
///
/// Regression: a producer that writes `let _ = get_or_compute(...)`
/// and unconditionally returns the locally-computed `resolved`
/// discards the `Some(cached)` value the concurrent-publish warm-hit
/// arm returns — so when the local slow-lane resolution produced
/// `None` (e.g. the exported name does not resolve) the producer
/// reported a spurious miss even though the authoritative cache held
/// a real symbol. This test pins the contract that the producer must
/// honour `Some(cached)` whenever the warm-hit arm fires.
///
/// The fixture pins the discriminator:
///
///  - The requested name (`Missing`) has NO resolution from
///    `/concurrent_pub/index.ts` — no local prepared decl, no resolvable
///    export target — so `resolve_imported_registry_symbol_with_budget`
///    deterministically produces a local `resolved` of `None`.
///  - A concurrent publish is injected: a real `ResolvedImportedRegistrySymbol`
///    is validated-and-published into `ImportedRegistryDb` after the
///    producer's `peek` miss and before its `get_or_compute`, so
///    `get_or_compute` takes the warm-hit arm and returns `Some(<that
///    symbol>)`.
///
/// Discrimination property — FAILS pre-fix, PASSES post-fix:
///
///  - Pre-fix (`let _ = get_or_compute(...)`): the `Some(cached)`
///    concurrent value is discarded; the producer returns the local
///    `resolved` (`None`). The request reports a spurious miss.
///  - Post-fix (`match host_value { Some(cached) => cached, None =>
///    resolved }`): the producer surfaces the authoritative
///    concurrently-published symbol.
#[test]
fn resolve_imported_registry_symbol_surfaces_concurrently_published_value() {
    use super::ResolvedImportedRegistrySymbol;

    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/concurrent_pub/index.ts".to_string(),
        Arc::from("export const anchor = 1;\n"),
    );
    ws.inject_file(
        "/concurrent_pub/types.ts".to_string(),
        Arc::from("export interface Props { primary: string }\n"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/concurrent_pub/index.ts"));
    assert!(host.ensure_loaded("/concurrent_pub/types.ts"));
    // Materialise the `IndexedReady` artifact for the keyed canonical
    // so the concurrent-publish injection below can build a
    // provenance-pure `fact_dep_signature` for the entry it plants.
    // `engine_fact_signature_for_exported_type` resolves the parse
    // facts content-addressed against the keyed canonical's observed
    // hash, which requires the `FileArtifactStore` artifact to exist.
    // In a real concurrent scenario the racing request that published
    // the entry would itself have materialised this artifact while
    // resolving the same key; the fixture reproduces that precondition
    // explicitly because the injection lands the entry BEFORE this
    // request's own resolution runs (inside the cooperative-admission
    // singleflight closure).
    assert!(host
        .ensure_indexed_ready("/concurrent_pub/index.ts")
        .is_some());

    let mut engine = ComponentMetaQueryEngine::new(&host);

    // The value a concurrent request validated-and-published into the
    // shared DB for the SAME key while this request was cold.
    let concurrent_symbol = ResolvedImportedRegistrySymbol {
        canonical_id: "/concurrent_pub/types.ts".to_string(),
        exported_name: "Props".to_string(),
        body: TypeExpr::Primitive(PrimitiveName::String),
        canonical_dependencies: std::collections::BTreeSet::new(),
    };
    let _publish =
        super::inject_imported_registry_concurrent_publish_for_tests(concurrent_symbol.clone());

    // `Missing` has no resolution from `/concurrent_pub/index.ts`, so
    // the producer's local slow-lane `resolved` is `None`. The injected
    // concurrent publish makes `get_or_compute` return `Some(<the
    // published symbol>)` via its warm-hit arm.
    let resolved = engine.resolve_imported_registry_symbol("/concurrent_pub/index.ts", "Missing");

    let resolved = resolved.expect(
        "the producer MUST surface the concurrently-published value `get_or_compute` returns \
         from its warm-hit arm — the pre-fix `let _ =` discarded it and returned the local \
         `None`, reporting a spurious miss",
    );
    assert_eq!(
        (
            resolved.canonical_id.as_str(),
            resolved.exported_name.as_str(),
        ),
        ("/concurrent_pub/types.ts", "Props"),
        "the surfaced value must be the concurrently-published symbol, not the local \
         unresolved `None`",
    );
}

/// Discriminating regression for the imported-registry singleflight
/// invariant: the slow-lane resolution must execute inside the
/// `ImportedRegistryDb` cooperative-admission closure so it runs at
/// most once per `(key, generation)`.
///
/// `resolve_imported_registry_symbol`'s expensive resolution —
/// `resolve_imported_registry_symbol_with_budget` — consumes the
/// per-request wildcard-route fuse (`allow_wildcard_route()` /
/// `wildcard_route_fanout`) on the slow lane. A prior implementation
/// moved that resolution OUTSIDE the `ImportedRegistryDb`
/// cooperative-admission closure. With the resolution outside the
/// singleflight slot, several requests that miss the cache for the
/// SAME key each run the resolution independently and each tick the
/// wildcard-route fuse — the one-winner contract documented for these
/// DBs is regressed.
///
/// The fix runs the resolution INSIDE the
/// `cooperative_admit_with_post_publish` `compute` closure, so exactly
/// one winner resolves under the `InflightTable` singleflight while
/// joiners block on the slot condvar and reuse the winner's value.
///
/// Driver shape — `WORKERS` real threads contend on one uncached
/// `ImportedRegistryDb` key:
///
///  - `Wide` is a re-export (`export { Inner as Wide } from './inner'`)
///    with NO local prepared declaration in `index.ts`, so
///    `resolve_imported_registry_symbol_with_budget` takes the slow
///    lane and consumes one `allow_wildcard_route()` tick per
///    invocation.
///  - The process-global post-peek barrier is armed for the keyed
///    canonical with `WORKERS` parties. Every worker blocks at the
///    seam AFTER its `peek` miss and BEFORE cooperative admission, so
///    all `WORKERS` workers are guaranteed past `peek` (all missing —
///    nothing is published yet) before any of them enters the
///    admission slot. This makes the discrimination deterministic in
///    BOTH configurations: it removes the timing window in which an
///    early publisher could let a late worker warm-hit `peek` and skip
///    its resolution.
///  - The process-global winner-park gate is armed for the same keyed
///    canonical. It closes the SECOND timing window the post-peek
///    barrier does not bound: the race INSIDE
///    `cooperative_admit_with_post_publish` between a worker's loop-top
///    `map.get` miss and its claim of the in-flight slot. Under load a
///    worker descheduled there can wake to a retired slot (the winner
///    already published AND retired it), fork a fresh slot, and become a
///    SECOND cold winner that ticks the wildcard-route fuse again. The
///    gate parks the cold winner inside its compute closure — after the
///    slot is claimed (`claimed == true` is published, forcing every
///    later arrival onto the joiner branch), before the resolution runs.
///    The main thread releases the winner only once it has PROVEN, via
///    the slot's `Arc` strong count (`3 + (WORKERS - 1) == WORKERS + 2`
///    while the winner is parked), that every joiner has coalesced onto
///    the winner's slot. No worker is then mid-flight between its miss
///    and its claim, so no second winner can form — `total_fuse_consumed`
///    is deterministically 1 even under the oversubscribed
///    `cargo test --workspace` gate. This rendezvous is the means;
///    weakening the exactly-once assertion is not.
///  - Each worker owns an independent `ComponentMetaQueryEngine` (hence
///    an independent wildcard-route fuse). After the join the test sums
///    each engine's observed fuse consumption.
///
/// Discrimination property — FAILS pre-fix, PASSES post-fix:
///
///  - Pre-fix (resolution outside the closure): every worker passes the
///    barrier, then every worker runs
///    `resolve_imported_registry_symbol_with_budget` before reaching
///    the inflight slot. The summed wildcard-route fuse consumption is
///    `WORKERS`, not 1.
///  - Post-fix (resolution inside the `cooperative_admit_with_post_publish`
///    closure): every worker passes the barrier, then exactly one
///    winner claims the inflight slot and runs the resolution; the
///    other `WORKERS - 1` block on the slot condvar and reuse the
///    winner's value. The summed wildcard-route fuse consumption is
///    exactly 1.
///
/// In both configurations every worker must receive the resolved
/// symbol — the singleflight fix must not regress correctness.
#[test]
fn resolve_imported_registry_symbol_resolves_once_under_concurrent_misses() {
    const WORKERS: usize = 8;

    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/singleflight/inner.ts".to_string(),
        Arc::from("export interface Inner { primary: string }"),
    );
    ws.inject_file(
        "/singleflight/index.ts".to_string(),
        Arc::from("export { Inner as Wide } from './inner'"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/singleflight/index.ts"));
    assert!(host.ensure_loaded("/singleflight/inner.ts"));
    // Wire the barrel's `./inner` re-export so the slow-lane
    // `resolve_named_type_export_target_shallow` route reaches the
    // defining file.
    host.set_import_dependencies(
        "/singleflight/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/singleflight/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Two-part deterministic singleflight rendezvous.
    //
    // First, the post-peek barrier: all WORKERS workers pass their `peek`
    // miss (nothing is published yet) before any enters cooperative
    // admission, so the test genuinely exercises WORKERS concurrent cold
    // misses on one key.
    //
    // Then, the winner park: the cold winner blocks inside the
    // cooperative-admission compute closure (after it claims the
    // in-flight slot, before it runs the fuse-consuming resolution). The
    // main thread releases it only once it has PROVEN — via the slot's
    // `Arc` strong count — that every other worker has coalesced onto the
    // winner's slot as a joiner. This closes the load-only window in
    // which a worker descheduled between its `map.get` miss and its slot
    // claim wakes to find the slot retired, forks a fresh slot, and
    // becomes a SECOND cold winner that ticks the wildcard-route fuse
    // again (the redundant-compute property the singleflight primitive
    // permits but this exactly-once discriminator must not observe).
    let (_barrier, _barrier_guard) =
        super::arm_imported_registry_post_peek_barrier_for_tests("/singleflight/index.ts", WORKERS);
    let (winner_release, _winner_park_guard) =
        super::arm_imported_registry_winner_park_for_tests("/singleflight/index.ts");

    // The keyed canonical the WORKERS cold misses contend on — used to
    // observe the in-flight slot's `Arc` strong count during the
    // coalescing rendezvous.
    let inflight_key: crate::component_meta_caches::ImportedRegistryKey = (
        std::sync::Arc::<str>::from("/singleflight/index.ts"),
        std::sync::Arc::<str>::from("Wide"),
    );

    let results: Vec<(usize, Option<super::ResolvedImportedRegistrySymbol>)> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..WORKERS)
                .map(|_| {
                    let host_ref = &host;
                    scope.spawn(move || {
                        let mut engine = ComponentMetaQueryEngine::new(host_ref);
                        // Give each worker's wildcard-route fuse ample
                        // budget: the discriminator is the SUMMED
                        // consumption count, not a near-fanout trip.
                        engine.prime_wildcard_route_fuse_for_tests(WORKERS + 4);
                        let fuse_before = engine.wildcard_route_fuse_consumed_for_tests();
                        let resolved = engine
                            .resolve_imported_registry_symbol("/singleflight/index.ts", "Wide");
                        let fuse_consumed = engine
                            .wildcard_route_fuse_consumed_for_tests()
                            .saturating_sub(fuse_before);
                        (fuse_consumed, resolved)
                    })
                })
                .collect();

            // Winner-park rendezvous — prove every joiner has coalesced
            // onto the winner's in-flight slot BEFORE releasing the winner.
            // While the winner is parked the substrate holds exactly
            // three `Arc`s on the slot (the in-flight table entry, the
            // winner's `slot` local, and the winner's `panic_guard.slot`);
            // each of the WORKERS-1 joiners bumps the count by one the
            // instant it clones its own `Arc` via the slot-acquisition
            // `table.entry(key).or_insert_with(..).clone()`, past which it
            // deterministically reaches the cooperative joiner wait branch
            // (the winner has already published `claimed == true`). The
            // target strong count is therefore 3 + (WORKERS - 1) ==
            // WORKERS + 2. `yield_now` (not a busy spin) keeps the main
            // thread from starving the very workers it is waiting on under
            // the oversubscribed gate.
            let db = host.project_type_store().imported_registry_db();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut coalesced = false;
            while std::time::Instant::now() < deadline {
                if db
                    .slot_strong_count_for_test(&inflight_key)
                    .is_some_and(|count| count >= WORKERS + 2)
                {
                    coalesced = true;
                    break;
                }
                std::thread::yield_now();
            }
            // Release the winner BEFORE joining so a coalescing timeout can
            // never deadlock the scope's thread join on the parked winner;
            // the `_winner_park_guard` is a drop-time backstop.
            winner_release.release();
            assert!(
                coalesced,
                "the {WORKERS} concurrent cold-miss workers failed to coalesce onto ONE \
                 in-flight slot within 30s — the deterministic singleflight rendezvous is \
                 broken (the winner-park / slot-strong-count contract no longer holds)",
            );

            handles
                .into_iter()
                .map(|h| h.join().expect("worker thread joined"))
                .collect()
        });

    let total_fuse_consumed: usize = results.iter().map(|(consumed, _)| consumed).sum();
    assert_eq!(
        total_fuse_consumed, 1,
        "the wildcard-route fuse MUST be consumed exactly ONCE across all {WORKERS} \
         concurrent cache misses for the same key — the singleflight contract requires \
         one winner to resolve while joiners reuse its value. The pre-fix producer ran \
         resolve_imported_registry_symbol_with_budget outside the cooperative-admission \
         closure, so every worker resolved independently and the summed fuse consumption \
         was {WORKERS}, not 1 (observed {total_fuse_consumed}).",
    );

    for (worker, (_, resolved)) in results.iter().enumerate() {
        let resolved = resolved.as_ref().unwrap_or_else(|| {
            panic!(
                "worker {worker} MUST receive the resolved imported symbol — the \
                 singleflight winner's value is broadcast to every joiner",
            )
        });
        assert_eq!(
            (
                resolved.canonical_id.as_str(),
                resolved.exported_name.as_str(),
            ),
            ("/singleflight/inner.ts", "Inner"),
            "worker {worker} must observe the slow-lane-resolved defining export, \
             identical to the singleflight winner's value",
        );
    }
}

// ── Dispatch-authoritative compound-root surfaces ──────────────────
//
// These tests drive the REAL root-surface bridge
// (`project_type_surface_expr_via_host_threaded`). The bridge carries NO
// prepared-decl root-surface rescue — dispatch is the sole root-surface
// authority — so each test proves the dispatch surface composition
// (`dispatch_projected_surface`, which composes compound roots from the
// decl anchor through the shared empty-path Shallow walker) produces a
// COMPLETE shallow surface for `Union` / `Intersection` (heritage) /
// `InstantiationRef` roots.
//
// Discrimination: with no fallback present, if dispatch fails to compose
// the compound root the bridge returns `None` and the `.expect` panics —
// the test FAILS. The member presence/absence assertions further pin the
// composed surface to the exact expected member set.

/// Collect property + method member names from a projected object surface
/// `TypeExpr`. Panics if `expr` is not an `Object` so a mis-shaped
/// projection fails loudly rather than silently asserting an empty set.
#[cfg(test)]
fn projected_object_member_names(expr: &TypeExpr) -> Vec<String> {
    let object = match expr {
        TypeExpr::Object(object) => object,
        other => panic!("expected projected Object surface, got {other:?}"),
    };
    let mut names = object
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.clone()),
            ObjectMember::Method(method) => Some(method.name.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn dispatch_authoritative_union_alias_surface_without_prepared_fallback() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    // Generic union alias: each arm shares `shared` and carries one
    // branch-only member. The dispatch compound-root composition drives
    // the shared shallow walker under the `MacroObjectSurface` context,
    // whose union merge enumerates the UNION of arm members — so all three
    // members appear, with branch-only members optional. A common-member
    // INTERSECTION merge would keep only `shared`; asserting the
    // branch-only members present discriminates union-of-members from
    // common-member-only.
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface ArmA {
  shared?: string
  onlyA?: number
}
export interface ArmB {
  shared?: string
  onlyB?: boolean
}
export type UnionAlias<T = ArmA> = ArmA | ArmB
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "UnionAlias",
    )
    .expect(
        "dispatch must compose the union-alias root surface (no prepared-decl fallback exists)",
    );

    let names = projected_object_member_names(&projected);
    assert!(
        names.iter().any(|name| name == "shared"),
        "union surface keeps the shared base member; got {names:?}",
    );
    assert!(
        names.iter().any(|name| name == "onlyA"),
        "union surface keeps ArmA's branch-only member (union-of-members, not common-member-only); got {names:?}",
    );
    assert!(
        names.iter().any(|name| name == "onlyB"),
        "union surface keeps ArmB's branch-only member (union-of-members, not common-member-only); got {names:?}",
    );
}

#[test]
fn dispatch_authoritative_generic_omit_heritage_surface_without_prepared_fallback() {
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
    // `ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'>`
    // where `SelectMenuProps<T> extends Pick<RootProps<T>, 'open' |
    // 'defaultOpen' | 'disabled'> { items?: T }`. The dispatch root is an
    // Intersection (heritage overlay over the `Omit<...>` carrier). The
    // dispatch compound-root composition must compose `open` / `defaultOpen`
    // / `disabled` (inherited via Pick) and OMIT `items`.
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "ColorModeSelectProps",
    )
    .expect(
        "dispatch must compose the generic Omit-heritage root surface (no prepared-decl fallback exists)",
    );

    let names = projected_object_member_names(&projected);
    for kept in ["open", "defaultOpen", "disabled"] {
        assert!(
            names.iter().any(|name| name == kept),
            "Omit-heritage surface keeps inherited `{kept}` (via Pick<RootProps>); got {names:?}",
        );
    }
    assert!(
        !names.iter().any(|name| name == "items"),
        "Omit<SelectMenuProps, 'items'> must drop `items` from the surface; got {names:?}",
    );
}

#[test]
fn dispatch_authoritative_ordinary_heritage_surface_without_prepared_fallback() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    // Ordinary (non-generic) heritage: `Derived extends ButtonProps`.
    // The dispatch root is an Intersection (heritage overlay). The
    // dispatch compound-root composition must compose the inherited
    // `disabled` and the derived own-body `extra`.
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface ButtonProps {
  disabled?: boolean
  label?: string
}
export interface Derived extends ButtonProps {
  extra?: number
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
        &mut query_engine,
        "/src/App.vue",
        "Derived",
    )
    .expect(
        "dispatch must compose the ordinary heritage root surface (no prepared-decl fallback exists)",
    );

    let names = projected_object_member_names(&projected);
    assert!(
        names.iter().any(|name| name == "disabled"),
        "ordinary heritage surface keeps inherited `disabled` from ButtonProps; got {names:?}",
    );
    assert!(
        names.iter().any(|name| name == "extra"),
        "ordinary heritage surface keeps the derived own-body `extra`; got {names:?}",
    );
}

// ── `projected_surface_to_type_expr` span re-emit (D1) ──────────────
// These two unit tests pin the span-threading invariants of the kept
// `projected_surface_to_type_expr` reconstruction directly from a
// hand-built `ProjectedSurface` — no walker, no dispatch. They build
// their input as a struct literal, so they characterise the
// reconstruction in isolation.

/// D1 span threading (positive): `projected_surface_to_type_expr` re-emits the
/// REAL member spans carried on `ProjectedMember` onto the reconstructed IR
/// property — it does NOT drop them to `MemberSpans::default()`.
///
/// Discriminating: reverting the reconstruction to pass `MemberSpans::default()`
/// (the pre-D1 state) makes every `Some(..)` below `None`, failing the asserts.
#[test]
fn projected_surface_to_type_expr_reemits_member_spans() {
    use crate::resolver_core::projected_surface_to_type_expr;
    use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
    use verter_span::Span;
    use verter_type_expr::MemberSpans;

    // A member carrying real OXC declaration-site spans (as the graph
    // `SurfaceMember` / `PreparedMember` / IR source would).
    let member = ProjectedMember {
        name: "label".to_string(),
        ty: TypeExpr::Primitive(PrimitiveName::String),
        optional: false,
        readonly: false,
        is_method: false,
        visibility: verter_type_expr::MemberVisibility::Public,
        declared_in_macro_type_arg: false,
        spans: MemberSpans {
            declaration: Some(Span::new(10, 24)),
            name: Some(Span::new(10, 15)),
            type_annotation: Some(Span::new(17, 23)),
        },
        declaration_origin: Some(std::sync::Arc::from("/decl.ts")),
    };
    let surface = ProjectedSurface {
        members: vec![member],
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: false,
    };

    let expr = projected_surface_to_type_expr(&surface)
        .expect("a one-member surface should reconstruct to an object type");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };
    let ObjectMember::Property(property) = &object.properties[0] else {
        panic!("expected a property member, got {:?}", object.properties[0]);
    };
    assert_eq!(
        property.spans.declaration,
        Some(Span::new(10, 24)),
        "the threaded declaration span must round-trip onto the IR property"
    );
    assert_eq!(
        property.spans.name,
        Some(Span::new(10, 15)),
        "the threaded name span must round-trip onto the IR property"
    );
    assert_eq!(
        property.spans.type_annotation,
        Some(Span::new(17, 23)),
        "the threaded type-annotation span must round-trip onto the IR property"
    );
}

/// F1 visibility threading: `projected_surface_to_type_expr` reconstructs a
/// member via `with_visibility` (NOT `with_spans`), so a non-public member
/// projected onto the `ProjectedSurface` survives the
/// SurfaceView -> ProjectedMember -> TypeExpr round-trip with its true
/// accessibility. This is both leak-prevention (the reconstructed surface must
/// not present a private member as public) and `native_props` fidelity.
///
/// Discriminating: against the tree where the reconstruction uses `with_spans`
/// (which defaults Public), the `Private` / `Protected` assertions below FAIL
/// (the reconstructed members are Public).
#[test]
fn projected_surface_to_type_expr_preserves_member_visibility() {
    use crate::resolver_core::projected_surface_to_type_expr;
    use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
    use verter_type_expr::{FunctionExpr, MemberSpans, MemberVisibility};

    let private_prop = ProjectedMember {
        name: "secret".to_string(),
        ty: TypeExpr::Primitive(PrimitiveName::Number),
        optional: false,
        readonly: false,
        is_method: false,
        visibility: MemberVisibility::Private,
        declared_in_macro_type_arg: false,
        spans: MemberSpans::default(),
        declaration_origin: None,
    };
    let protected_method = ProjectedMember {
        name: "guarded".to_string(),
        ty: TypeExpr::Function(std::sync::Arc::new(FunctionExpr::synthetic(
            Vec::new(),
            None,
            Vec::new(),
        ))),
        optional: false,
        readonly: false,
        is_method: true,
        visibility: MemberVisibility::Protected,
        declared_in_macro_type_arg: false,
        spans: MemberSpans::default(),
        declaration_origin: None,
    };
    let public_prop = ProjectedMember {
        name: "open".to_string(),
        ty: TypeExpr::Primitive(PrimitiveName::String),
        optional: false,
        readonly: false,
        is_method: false,
        visibility: MemberVisibility::Public,
        declared_in_macro_type_arg: false,
        spans: MemberSpans::default(),
        declaration_origin: None,
    };

    let surface = ProjectedSurface {
        members: vec![private_prop, protected_method, public_prop],
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: false,
    };

    let expr = projected_surface_to_type_expr(&surface).expect("multi-member surface reconstructs");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };

    let find_property = |name: &str| -> MemberVisibility {
        object
            .properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Property(p) if p.name == name => Some(p.visibility),
                _ => None,
            })
            .unwrap_or_else(|| panic!("property `{name}` must be reconstructed"))
    };
    let find_method = |name: &str| -> MemberVisibility {
        object
            .properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Method(m) if m.name == name => Some(m.visibility),
                _ => None,
            })
            .unwrap_or_else(|| panic!("method `{name}` must be reconstructed"))
    };

    assert_eq!(
        find_property("secret"),
        MemberVisibility::Private,
        "a private projected property must reconstruct as Private",
    );
    assert_eq!(
        find_method("guarded"),
        MemberVisibility::Protected,
        "a protected projected method must reconstruct as Protected",
    );
    assert_eq!(
        find_property("open"),
        MemberVisibility::Public,
        "a public projected property stays Public",
    );
}

/// D1 span threading (negative): the GENUINELY synthetic open-surface index
/// signature stays span-`None`. `ProjectedSurface` carries only a
/// `has_index_signature: bool` — no declared key/value nodes, hence no single
/// OXC declaration site — so the reconstruction must NOT fabricate spans.
///
/// Discriminating: a reconstruction that fabricated a non-`None` span here
/// (e.g. a byte-0 placeholder) would fail these `None` asserts.
#[test]
fn projected_surface_to_type_expr_keeps_synthetic_index_signature_span_none() {
    use crate::resolver_core::projected_surface_to_type_expr;
    use verter_semantic::analysis::type_solver::query_engine::ProjectedSurface;

    let surface = ProjectedSurface {
        members: Vec::new(),
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: true,
    };

    let expr = projected_surface_to_type_expr(&surface)
        .expect("an open surface should reconstruct to an object type");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };
    let index = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::IndexSignature(sig) => Some(sig),
            _ => None,
        })
        .expect("open surface must reconstruct an index signature");
    assert_eq!(
        index.spans.declaration, None,
        "synthetic open-surface index signature has no source site — must NOT fabricate a span"
    );
    assert_eq!(
        index.spans.key, None,
        "synthetic open-surface index signature key has no source site"
    );
    assert_eq!(
        index.spans.value, None,
        "synthetic open-surface index signature value has no source site"
    );
}

/// DISCRIMINATING (fix-cycle-4 Site A — `route_keys.rs`
/// `enumerate_member_surface_keys_via_route`): a `keyof X['member']` route key
/// source where `member` is a NON-PUBLIC (private/protected) class member must
/// NOT derive route keys from that member. `keyof C` excludes
/// protected/private members (TS: `keyof ClassType` is public-only), so
/// `keyof C['privateObj']` may not be reached by external indexed access — the
/// member's surface keys must not feed the Pick/Omit route key set.
///
/// Discrimination: FAILS on the pre-fix tree where the `TypeExpr::Object` arm
/// of `enumerate_member_surface_keys_via_route` matches the member by NAME
/// only (`property.name == member_name` / `method.name == member_name`) with no
/// visibility gate — `privateObj` matches, its `{ secretKey }` surface is
/// projected, and `["secretKey"]` leaks into the key set. PASSES once the arm
/// requires `visibility.is_public()` (a non-public match is a miss).
#[test]
fn enumerate_member_surface_keys_via_route_gates_non_public_member() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export class C {
  public publicObj: { openKey: string } = { openKey: "" }
  private privateObj: { secretKey: string } = { secretKey: "" }
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

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // Positive control: `keyof C['publicObj']` (PUBLIC member) enumerates its
    // surface keys.
    let public_source = TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("C")),
        index: Arc::new(TypeExpr::string_literal("publicObj")),
    }));
    let public_keys = query_engine
        .enumerate_route_literal_keys("/src/App.vue", "/src/App.vue", &public_source)
        .expect(
            "positive control: `keyof C['publicObj']` (public member) MUST enumerate its surface \
             keys (Some) — a None here means the route-key enumeration regressed, which would let \
             the negative assertion below pass vacuously",
        );
    assert!(
        public_keys.iter().any(|k| k == "openKey"),
        "keyof C['publicObj'] (public member) must enumerate `openKey`: {public_keys:?}"
    );

    // DISCRIMINATING: `keyof C['privateObj']` (PRIVATE member) must NOT derive
    // route keys from the non-public member's surface.
    let private_source = TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("C")),
        index: Arc::new(TypeExpr::string_literal("privateObj")),
    }));
    let private_keys =
        query_engine.enumerate_route_literal_keys("/src/App.vue", "/src/App.vue", &private_source);
    if let Some(keys) = private_keys {
        assert!(
            !keys.iter().any(|k| k == "secretKey"),
            "keyof C['privateObj'] (non-public member) must NOT derive route keys from the \
             private member's surface, got {keys:?}"
        );
    }
}

/// DISCRIMINATING (fix-cycle-4 Site A end-to-end): `Pick<C, keyof
/// C['privateObj']>` projected through `project_direct_utility_surface_shape`
/// must publish NOTHING — the key set derives from a non-public member's
/// surface, which is not on `keyof C`.
///
/// Discrimination: FAILS pre-fix (the private member's `secretKey` enters the
/// Pick key set and — if `C` carried a same-named public member — would be
/// retained). PASSES once the route-key enumeration gates non-public members.
#[test]
fn pick_with_keyof_indexed_private_member_keys_publishes_nothing() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export class C {
  public secretKey: string = ""
  private privateObj: { secretKey: string } = { secretKey: "" }
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

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    // `Pick<C, keyof C['privateObj']>` — the key set is `keyof { secretKey }`
    // = `'secretKey'`, derived from the PRIVATE `privateObj` member. `C` also
    // has a PUBLIC `secretKey` member whose name collides. If the private
    // member's surface is allowed to feed the key set, `secretKey` would be
    // picked. The non-public-derived key must NOT select the public member.
    let expr = TypeExpr::named_with_args(
        "Pick",
        vec![
            TypeExpr::named("C"),
            TypeExpr::KeyOf(Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("C")),
                index: Arc::new(TypeExpr::string_literal("privateObj")),
            })),
        ],
    );

    let shape = query_engine.project_direct_utility_surface_shape("/src/App.vue", &expr);
    if let Some(shape) = shape {
        let names: std::collections::BTreeSet<&str> = shape
            .properties
            .iter()
            .map(|property| property.name.as_str())
            .collect();
        assert!(
            !names.contains("secretKey"),
            "Pick<C, keyof C['privateObj']> must not select a key derived from the non-public \
             `privateObj` surface: {names:?}"
        );
    }
}

/// DISCRIMINATING (fix-cycle-4 Site B — `route_keys.rs`
/// `project_direct_utility_surface_shape` nested-utility fallback):
/// `Pick<Partial<C>, 'c'>` / `Omit<Partial<C>, 'a'>` over a class `C` with a
/// PRIVATE / PROTECTED member must NOT leave the non-public member on the
/// projected utility surface. `Partial<C>` keeps every member name, then the
/// local Pick/Omit fallback `retain`s by name — without a public gate, a
/// non-public member survives.
///
/// Discrimination: FAILS on the pre-fix tree where the `Pick`/`Omit` arms of
/// `project_direct_utility_surface_shape` filter the `ExpandedObjectShape` by
/// NAME only (`:627` / `:641`) and the shape's `ExpandedProperty` carries no
/// visibility — `Pick<Partial<C>, 'c'>` retains the private `c`. PASSES once
/// `ExpandedProperty` carries visibility and the `retain` gates on
/// `visibility.is_public()`.
#[test]
fn project_direct_utility_pick_omit_over_partial_class_excludes_non_public() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
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

    let _store_view = host.resolver_store_view();
    let mut query_engine = ComponentMetaQueryEngine::new(&host);

    let names_of = |shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape| {
        shape
            .properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<std::collections::BTreeSet<String>>()
    };

    // DISCRIMINATING: `Pick<Partial<C>, 'c'>` over a PRIVATE key.
    let pick_private = TypeExpr::named_with_args(
        "Pick",
        vec![
            TypeExpr::named_with_args("Partial", vec![TypeExpr::named("C")]),
            TypeExpr::string_literal("c"),
        ],
    );
    if let Some(shape) =
        query_engine.project_direct_utility_surface_shape("/src/App.vue", &pick_private)
    {
        assert!(
            !names_of(&shape).contains("c"),
            "Pick<Partial<C>, 'c'> (private `c`) must NOT keep `c`: {:?}",
            names_of(&shape)
        );
    }

    // DISCRIMINATING: `Omit<Partial<C>, 'a'>` over the PUBLIC key `a` must NOT
    // leave the non-public `b` / `c` on the surface (keyof C is public-only =
    // `'a'`; omitting it leaves an empty public keyspace).
    let omit_public = TypeExpr::named_with_args(
        "Omit",
        vec![
            TypeExpr::named_with_args("Partial", vec![TypeExpr::named("C")]),
            TypeExpr::string_literal("a"),
        ],
    );
    if let Some(shape) =
        query_engine.project_direct_utility_surface_shape("/src/App.vue", &omit_public)
    {
        let names = names_of(&shape);
        assert!(
            !names.contains("b") && !names.contains("c"),
            "Omit<Partial<C>, 'a'> must NOT leave the non-public `b` / `c` published: {names:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────
// Wildcard-route fuse-trip is a PARTIAL, not an absent.
// A route-only symbol whose wildcard-route fuse is exhausted was NEVER
// looked up: its `None` MUST NOT admit an ImportedRegistryDb warm
// negative, and the derived ResolvabilityDb `false` MUST NOT be cached.
// ─────────────────────────────────────────────────────────────────

/// Build the route-only re-export fixture: `ButtonProps` is
/// re-exported from `./types` with NO local prepared declaration in
/// `index.ts`, so resolving it takes the slow wildcard-route lane and
/// consumes one `allow_wildcard_route()` tick.
fn build_route_only_reexport_host() -> Arc<VerterHost> {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/m4_src/types.ts".to_string(),
        Arc::from("export interface Props { primary: string }"),
    );
    ws.inject_file(
        "/m4_src/index.ts".to_string(),
        Arc::from("export { Props as ButtonProps } from './types'"),
    );
    let host = Arc::new(VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    ));
    assert!(host.ensure_loaded("/m4_src/index.ts"));
    assert!(host.ensure_loaded("/m4_src/types.ts"));
    host.set_import_dependencies(
        "/m4_src/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/m4_src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host
}

/// A route-only symbol whose wildcard-route fuse is primed to
/// ZERO trips on the slow lane — the symbol is NEVER looked up. The
/// fuse-trip `None` MUST NOT admit an ImportedRegistryDb warm negative,
/// and a FRESH engine WITH budget MUST resolve the symbol.
///
/// MUTATION CHECK: reverting the fuse-trip gate (collapsing FuseTripped
/// back to a plain `None` resolved value that falls through to
/// `Cacheable`) admits a
/// warm negative — the `imported_registry_db().live_count()` assertion
/// fails (a non-zero negative is cached).
#[test]
fn fuse_tripped_route_only_symbol_admits_no_warm_negative_and_fresh_resolves() {
    let host = build_route_only_reexport_host();
    // Materialise the keyed canonical's artifact so admission would have
    // a provenance-pure signature available — isolating the refusal to
    // the fuse-trip path, not a missing-artifact path.
    assert!(host.ensure_indexed_ready("/m4_src/index.ts").is_some());

    let imported_db = host.project_type_store().imported_registry_db();
    let neg_before = imported_db.live_count();

    // Request 1 — fuse primed to 0: the slow-lane `allow_wildcard_route()`
    // trips immediately, so the symbol is never looked up (PARTIAL).
    {
        let mut engine = ComponentMetaQueryEngine::new(host.as_ref());
        engine.prime_wildcard_route_fuse_for_tests(0);
        let resolved = engine.resolve_imported_registry_symbol("/m4_src/index.ts", "ButtonProps");
        assert!(
            resolved.is_none(),
            "the fuse-tripped request returns no symbol (the route was never taken)",
        );
        assert!(
            engine.has_fuse_tripped(),
            "fixture invariant: the wildcard-route fuse tripped on the route-only symbol",
        );
    }

    assert_eq!(
        imported_db.live_count(),
        neg_before,
        "a wildcard-route fuse-trip MUST NOT admit an ImportedRegistryDb warm \
         negative (the symbol was never looked up; admitting `value:None` would poison \
         a later budgeted request) — live count must be unchanged",
    );

    // Request 2 — FRESH engine WITH budget. The symbol resolves (proving
    // no warm negative short-circuited it to None).
    let mut engine2 = ComponentMetaQueryEngine::new(host.as_ref());
    let resolved2 = engine2.resolve_imported_registry_symbol("/m4_src/index.ts", "ButtonProps");
    let resolved2 = resolved2.expect(
        "a fresh request WITH wildcard-route budget MUST resolve the route-only symbol — \
         a cached fuse-trip negative would spuriously short-circuit it to None",
    );
    assert_eq!(
        (
            resolved2.canonical_id.as_str(),
            resolved2.exported_name.as_str(),
        ),
        ("/m4_src/types.ts", "Props"),
        "the budgeted request must resolve to the defining export",
    );
}

/// `can_resolve_registry_symbol` for a route-only symbol whose
/// wildcard-route fuse is exhausted MUST NOT cache the derived `false`
/// into ResolvabilityDb (the imported-registry call set the partial
/// sticky), and a fresh budgeted request MUST report it resolvable.
///
/// MUTATION CHECK: reverting the partial-sticky gate (removing it before
/// the ResolvabilityDb `Cacheable` admission) caches the derived `false`
/// — the `resolvable_db().live_count()` assertion fails.
#[test]
fn fuse_tripped_resolvability_does_not_cache_derived_false() {
    let host = build_route_only_reexport_host();
    assert!(host.ensure_indexed_ready("/m4_src/index.ts").is_some());

    let resolvable_db = host.project_type_store().resolvable_db();
    let false_before = resolvable_db.live_count();

    // Request 1 — fuse primed to 0: the imported-registry resolution
    // trips the fuse and sets the request partial sticky, so the derived
    // `false` MUST NOT be admitted. A `RequestContext` is installed
    // because the partial sticky is request-scoped (production
    // component-meta requests always install one before resolving).
    {
        use crate::request_context::{RequestContext, RequestContextGuard};
        let rctx = RequestContext::new(1, Arc::from("/m4_src/index.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        let mut engine = ComponentMetaQueryEngine::new(host.as_ref());
        engine.prime_wildcard_route_fuse_for_tests(0);
        let _ = engine.can_resolve_registry_symbol("/m4_src/index.ts", "ButtonProps", None);
        assert!(
            engine.has_fuse_tripped(),
            "fixture invariant: the wildcard-route fuse tripped during resolvability",
        );
        assert!(
            crate::request_context::current_materialization_cache_suppress(),
            "precondition: the fuse-trip MUST set the request partial sticky",
        );
    }

    assert_eq!(
        resolvable_db.live_count(),
        false_before,
        "a wildcard-route fuse-trip MUST NOT cache the derived ResolvabilityDb `false` \
         (the symbol was never looked up) — live count must be unchanged",
    );

    // Request 2 — FRESH engine WITH budget. The symbol IS resolvable.
    let mut engine2 = ComponentMetaQueryEngine::new(host.as_ref());
    let resolvable2 = engine2.can_resolve_registry_symbol("/m4_src/index.ts", "ButtonProps", None);
    assert!(
        resolvable2,
        "a fresh request WITH wildcard-route budget MUST report the route-only symbol \
         resolvable — a cached fuse-trip `false` would spuriously report it unresolvable",
    );
}
