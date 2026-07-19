//! Inline tests for `ComponentMetaQueryEngine` extracted from
//! `component_meta_query_engine/mod.rs`.
//!
//! This module is gated behind `#[cfg(test)]` via the parent's
//! `mod tests;` declaration. Tests reference parent-private items
//! through `super::<name>`; the parent re-exports the necessary
//! engine-impl methods as `pub(super)` from sibling modules so the
//! tests resolve symmetrically regardless of which sibling
//! `impl<'a> ComponentMetaQueryEngine<'a>` block defined the method.
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
        .resolve_direct_prepared_type_declaration(
            "/src/Avatar.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "AvatarProps",
        )
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
        .resolve_direct_prepared_type_declaration_metadata(
            "/src/Avatar.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "AvatarProps",
        )
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

    let analysis = host
        .get_analysis("/src/App.vue")
        .expect("App.vue analysis snapshot");
    let payload = analysis.macros[0]
        .prop_fields
        .iter()
        .find(|field| field.name == "title")
        .and_then(|field| field.payload.clone())
        .unwrap_or_else(|| panic!("the analyzer must stamp the title field's payload locator"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let field_value = engine
        .macro_field_value_node(
            "/src/App.vue",
            0,
            &[verter_semantic::analysis::type_eval_build::PathSegment::Member(Arc::from("title"))],
        )
        .expect("title field value node");
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &payload, field_value)
        .expect("local aliases that wrap package refs should use the fast shallow path");

    let shallow = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/src/App.vue",
        &fast.semantic_source,
    )
    .unwrap_or_else(|| panic!("the fast path's published source must shell-materialize"));
    let TypeExpr::Union(members) = &shallow else {
        panic!("local alias fast path should expand to the alias body, got {shallow:?}");
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

    let analysis = host
        .get_analysis("/src/App.vue")
        .expect("App.vue analysis snapshot");
    let payload = analysis.macros[0]
        .prop_fields
        .iter()
        .find(|field| field.name == "content")
        .and_then(|field| field.payload.clone())
        .unwrap_or_else(|| panic!("the analyzer must stamp the content field's payload locator"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let field_value = engine
        .macro_field_value_node(
            "/src/App.vue",
            0,
            &[
                verter_semantic::analysis::type_eval_build::PathSegment::Member(Arc::from(
                    "content",
                )),
            ],
        )
        .expect("content field value node");
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &payload, field_value)
        .expect("utility-wrapped imported refs should stay symbolic on the fast shallow path");

    assert_eq!(
        fast.hot.node(),
        field_value.node(),
        "utility-wrapped imported refs should remain symbolic in fast shallow expansion — the \
         published carrier is the unexpanded field value",
    );
    assert_eq!(
        fast.semantic_source,
        verter_type_expr::facts::SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload.clone()),
        ),
        "the symbolic fast path publishes the field's authored macro-payload source",
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

    let analysis = host
        .get_analysis("/src/App.vue")
        .expect("App.vue analysis snapshot");
    let payload = analysis.macros[0]
        .prop_fields
        .iter()
        .find(|field| field.name == "contentId")
        .and_then(|field| field.payload.clone())
        .unwrap_or_else(|| panic!("the analyzer must stamp the contentId field's payload locator"));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let field_value = engine
        .macro_field_value_node(
            "/src/App.vue",
            0,
            &[
                verter_semantic::analysis::type_eval_build::PathSegment::Member(Arc::from(
                    "contentId",
                )),
            ],
        )
        .expect("contentId field value node");
    let fast = engine
        .try_fast_shallow_field_expr("/src/App.vue", &payload, field_value)
        .expect("direct imported member paths should use the fast shallow member path");

    let materialized = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/src/App.vue",
        &fast.semantic_source,
    )
    .unwrap_or_else(|| panic!("the fast path's member source must demand-materialize"));
    assert_eq!(
        materialized,
        TypeExpr::Primitive(PrimitiveName::String),
        "direct imported member paths should materialize the prepared member body",
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
        crate::resolver_core::RouteDemand::pick(vec!["to".to_string(), "target".to_string()]);

    let projected = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
            &route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
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
    // routed Pick/Omit path (`dispatch_routed_expr_surface_node`) must NOT
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
    let pub_route = crate::resolver_core::RouteDemand::pick(vec!["open".to_string()]);
    if let Some(projected) = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Mixed.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "MixedClass",
            &pub_route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
    {
        assert!(
            member_names(&projected).contains("open"),
            "Pick<MixedClass, \"open\"> (public) must materialise `open`: {projected:?}"
        );
    }

    // DISCRIMINATING: Pick of a PRIVATE / PROTECTED key must NOT materialise it.
    for key in ["secret", "guarded"] {
        let route = crate::resolver_core::RouteDemand::pick(vec![key.to_string()]);
        let projected = query_engine
            .dispatch_routed_expr_surface_node(
                "/src/Mixed.vue",
                verter_type_expr::TopLevelOwnerId::module(0),
                "MixedClass",
                &route,
            )
            .and_then(|node| {
                super::surface::materialize_route_projection_node(query_engine.ctx, &node)
            });
        if let Some(projected) = projected {
            assert!(
                !member_names(&projected).contains(key),
                "routed Pick<MixedClass, \"{key}\"> (non-public) must NOT materialise `{key}`: {projected:?}"
            );
        }
    }

    // DISCRIMINATING: Omit of the PUBLIC key must NOT leave the non-public
    // members on the routed surface.
    let omit_route = crate::resolver_core::RouteDemand::omit(vec!["open".to_string()]);
    if let Some(projected) = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Mixed.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "MixedClass",
            &omit_route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
    {
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
        crate::resolver_core::RouteDemand::pick(vec!["to".to_string(), "target".to_string()]);

    let projected = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
            &route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
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
        crate::resolver_core::RouteDemand::pick(vec!["to".to_string(), "target".to_string()]);

    let projected = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
            &route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
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
        crate::resolver_core::RouteDemand::pick(vec!["target".to_string(), "to".to_string()]);

    let projected = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
            &route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
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
        crate::resolver_core::RouteDemand::pick(vec!["target".to_string(), "to".to_string()]);

    let projected = query_engine
        .dispatch_routed_expr_surface_node(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
            &route,
        )
        .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
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

    // Instantiate through the shared dispatch Expanded lowering (the same path
    // the route-key leaf stabiliser drives): lower `F<string>` at Expanded so
    // `build_instantiate` binds the outer `T -> string` and substitutes while
    // lowering, then materialise the instantiated function body once.
    let instantiated = crate::resolver_core::lower_and_project_to_expanded_node(
        query_engine.ctx,
        "/src/App.vue",
        verter_type_expr::TopLevelOwnerId::instance(0),
        &expr,
    )
    .and_then(|node| super::surface::materialize_route_projection_node(query_engine.ctx, &node))
    .expect(
        "F<string> over a function-typed generic alias should instantiate to the function body \
         via the shared dispatch instantiation path",
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
    let color_ty = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/src/Alert.vue",
        color_prop.type_source.present().expect("typed color prop"),
    )
    .unwrap_or_else(|| panic!("color's published source must demand-materialize"));
    let is_resolved_color = matches!(&color_ty, TypeExpr::Union(_) | TypeExpr::Literal(_));
    assert!(
        is_resolved_color,
        "color prop should resolve to a literal union, got {color_ty:?}",
    );

    // Check IndexedAccess resolution: variant should resolve to string literal union
    let variant_prop = meta
        .props
        .iter()
        .find(|p| p.name == "variant")
        .expect("should have variant prop");
    let variant_ty = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/src/Alert.vue",
        variant_prop
            .type_source
            .present()
            .expect("typed variant prop"),
    )
    .unwrap_or_else(|| panic!("variant's published source must demand-materialize"));
    let is_resolved_variant = matches!(&variant_ty, TypeExpr::Union(_) | TypeExpr::Literal(_));
    assert!(
        is_resolved_variant,
        "variant prop should resolve to a literal union, got {variant_ty:?}",
    );

    // Imported Props-like refs stay symbolic in the native API — the compat
    // layer expands them in the schema field while the type string preserves
    // the named form (e.g. "AvatarProps | undefined").
    let avatar_prop = meta
        .props
        .iter()
        .find(|p| p.name == "avatar")
        .expect("should have avatar prop");
    let avatar_ty = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/src/Alert.vue",
        avatar_prop
            .type_source
            .present()
            .expect("typed avatar prop"),
    )
    .unwrap_or_else(|| panic!("avatar's published source must shell-materialize"));
    assert!(
        matches!(
            &avatar_ty,
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
        ),
        "avatar prop should stay as symbolic Ref('AvatarProps'), got {avatar_ty:?}",
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
    // Membership rides the shared file-scoped extraction
    // (`FactVersionRef::canonical_id`) so the assertion holds
    // regardless of which fact variant carries the helper canonical.
    let helper_referenced = resolved
        .fact_versions
        .iter()
        .any(|fact| fact.canonical_id() == Some("/src/Helper.ts"));

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

    use std::sync::Arc;
    use verter_semantic::analysis::Hash16;

    let analysis = Arc::new(
        verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource::default(),
    );
    let state = ShallowFileState::header_routing_only_for_test(Hash16::default(), analysis);

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
            membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                &verter_workspace::CanonicalPath::new("/workspace/node_modules/@me/inner-pkg"),
            ),
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

    let resolved = engine.resolve_imported_registry_symbol(
        "/src/index.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "ButtonProps",
    );

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
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        exported_name: "Props".to_string(),
        body: verter_type_expr::facts::PreparedTypeBodyFacts {
            classification: verter_type_expr::facts::TypeBodyClass::Interface,
            body_slot: verter_type_expr::locators::TypeBodySlot {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from("/concurrent_pub/types.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from("Props"),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            merged_contributor_slots: Arc::from(Vec::new().into_boxed_slice()),
        },
        canonical_dependencies: std::collections::BTreeSet::new(),
    };
    let _publish =
        super::inject_imported_registry_concurrent_publish_for_tests(concurrent_symbol.clone());

    // `Missing` has no resolution from `/concurrent_pub/index.ts`, so
    // the producer's local slow-lane `resolved` is `None`. The injected
    // concurrent publish makes `get_or_compute` return `Some(<the
    // published symbol>)` via its warm-hit arm.
    let resolved = engine.resolve_imported_registry_symbol(
        "/concurrent_pub/index.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Missing",
    );

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
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
                        let resolved = engine.resolve_imported_registry_symbol(
                            "/singleflight/index.ts",
                            verter_type_expr::TopLevelOwnerId::ordinary_file(),
                            "Wide",
                        );
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
        let resolved = engine.resolve_imported_registry_symbol(
            "/m4_src/index.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ButtonProps",
        );
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
    let resolved2 = engine2.resolve_imported_registry_symbol(
        "/m4_src/index.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "ButtonProps",
    );
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
        let _ = engine.can_resolve_registry_symbol(
            "/m4_src/index.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ButtonProps",
            None,
        );
        assert!(
            engine.has_fuse_tripped(),
            "fixture invariant: the wildcard-route fuse tripped during resolvability",
        );
        assert!(
            crate::request_context::current_request_result_is_partial(),
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
    let resolvable2 = engine2.can_resolve_registry_symbol(
        "/m4_src/index.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "ButtonProps",
        None,
    );
    assert!(
        resolvable2,
        "a fresh request WITH wildcard-route budget MUST report the route-only symbol \
         resolvable — a cached fuse-trip `false` would spuriously report it unresolvable",
    );
}

/// BLK4 (inner-cache fenced-serve poison) — `ResolvabilityDb` must REFUSE
/// admission when the resolvability compute (`resolve_imported_registry_symbol`)
/// consumed a FENCED (ReturnOnly, `store_published == false`) serve. The
/// verdict stays `Complete` (a fenced serve is non-cacheable, NOT partial), so
/// the compute's `refuse_result_cache_admission_if_partial` gate — which only
/// catches the wildcard-route FUSE trip — does NOT fire. The ONLY rail that
/// refuses the poisoned verdict is the nested fact tracer (the
/// `RefCycleResultDb` / `app_config_no_override_proof` sibling pattern), which
/// pre-fix `can_resolve_registry_symbol` never installed — admitting a verdict
/// derived from a served-without-publication basis whose facts validate live.
///
/// DISCRIMINATING: `force_indexed_ready_serve_fence_for_tests` fences every
/// `ensure_indexed_ready_serve` the compute drives at a STABLE generation (no
/// bump — so a `GenerationSuperseded` admission gate cannot mask the refusal,
/// and the served `indexed` still resolves the verdict). The unfenced control
/// admits the verdict (`live_count` grows); the fenced request must NOT
/// (`live_count` unchanged) while the request stays `Complete`. RED-pre (no
/// nested tracer) the fenced verdict LANDS in `ResolvabilityDb` and a later
/// same-generation warm hit inherits the stale verdict.
#[test]
fn fenced_serve_resolvability_verdict_is_not_admitted() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::sync::atomic::Ordering;

    // Control — an UNFENCED resolvability query admits the verdict.
    let control = build_route_only_reexport_host();
    assert!(control.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let control_db = control.project_type_store().resolvable_db();
    let control_before = control_db.live_count();
    {
        let rctx = RequestContext::new(1, Arc::from("/m4_src/index.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        let mut engine = ComponentMetaQueryEngine::new(control.as_ref());
        assert!(
            engine.can_resolve_registry_symbol(
                "/m4_src/index.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "ButtonProps",
                None,
            ),
            "fixture invariant: the route-only symbol resolves",
        );
    }
    assert!(
        control_db.live_count() > control_before,
        "control: an unfenced resolvability query admits the verdict into ResolvabilityDb \
         (fixture invariant — otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` inside the compute is fenced at
    // a STABLE generation, so the route resolution consumes a served-without-
    // publication artifact while its facts validate against the live view.
    let host = build_route_only_reexport_host();
    assert!(host.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let db = host.project_type_store().resolvable_db();
    let before = db.live_count();
    {
        let rctx = RequestContext::new(1, Arc::from("/m4_src/index.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        let mut engine = ComponentMetaQueryEngine::new(host.as_ref());
        host.test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let _ = engine.can_resolve_registry_symbol(
            "/m4_src/index.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ButtonProps",
            None,
        );
        host.test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        // Orthogonality: a fenced serve is non-cacheable, NOT partial — it must
        // NOT raise the request partial sticky.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced resolvability serve is non-cacheable, NOT partial — non-cacheability \
             routes through the fact tracer, never the partial sticky",
        );
    }
    assert_eq!(
        db.live_count(),
        before,
        "POISON: a fenced (non-cacheable) resolvability compute admitted its verdict into \
         ResolvabilityDb — the nested fact tracer (RefCycleResultDb / app_config sibling \
         pattern) must refuse admission, else a later same-generation warm hit inherits the \
         stale verdict derived from a served-without-publication basis",
    );
}

/// The SAME `ResolvabilityDb` admission boundary must ALSO refuse on a tracer
/// `FactReadSetFinalise::Overflow` — the SECOND, independent non-admission
/// condition. The entry's signature is built from the keyed canonical's observed
/// hash, NOT from this tracer's finalised set, so an overflow seen only by the
/// tracer would otherwise be dropped on the floor and a rootless verdict would warm
/// the shared cache (a signature above `FACT_SIGNATURE_CAP` can root NO entry, so a
/// warm read could never revalidate it).
///
/// DISCRIMINATING: the per-host overflow knob fans `FACT_SIGNATURE_CAP + 1`
/// synthetic observations into every installed tracer, so the resolvability
/// compute's tracer finalises `Overflow` with NO fenced serve and NO partial — the
/// exact state the pre-fix boundary (which read only `non_cacheable_read_observed`
/// and discarded the finalise) ADMITTED. The unfenced/unarmed control admits
/// (`live_count` grows); the overflowed compute must NOT.
#[test]
fn tracer_overflow_refuses_resolvability_verdict_admission() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::sync::atomic::Ordering;

    // Control — an unarmed resolvability query admits the verdict.
    let control = build_route_only_reexport_host();
    assert!(control.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let control_db = control.project_type_store().resolvable_db();
    let control_before = control_db.live_count();
    {
        let rctx = RequestContext::new(1, Arc::from("/m4_src/index.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        let mut engine = ComponentMetaQueryEngine::new(control.as_ref());
        assert!(
            engine.can_resolve_registry_symbol(
                "/m4_src/index.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "ButtonProps",
                None,
            ),
            "fixture invariant: the route-only symbol resolves",
        );
    }
    assert!(
        control_db.live_count() > control_before,
        "control: an unarmed resolvability query admits the verdict into ResolvabilityDb \
         (fixture invariant — otherwise the overflow assertion is vacuous)",
    );

    // Overflowed — the compute's tracer observes above the cap, so no signature can
    // root the verdict.
    let host = build_route_only_reexport_host();
    assert!(host.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let db = host.project_type_store().resolvable_db();
    let before = db.live_count();
    {
        let rctx = RequestContext::new(1, Arc::from("/m4_src/index.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        let mut engine = ComponentMetaQueryEngine::new(host.as_ref());
        host.test_force
            .force_fact_tracer_overflow_observations
            .store(
                crate::resolver_core::FACT_SIGNATURE_CAP + 1,
                Ordering::Relaxed,
            );
        let resolvable = engine.can_resolve_registry_symbol(
            "/m4_src/index.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ButtonProps",
            None,
        );
        host.test_force
            .force_fact_tracer_overflow_observations
            .store(0, Ordering::Relaxed);
        // The verdict still flows to THIS caller (overflow is cache-only).
        assert!(
            resolvable,
            "an overflowed resolvability compute still SERVES its verdict to the caller — \
             overflow refuses cache admission, it does not degrade the answer",
        );
        // Orthogonality: overflow is non-cacheable, NOT partial.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a tracer overflow is non-cacheable, NOT partial — it must never raise the \
             request partial sticky",
        );
    }
    assert_eq!(
        db.live_count(),
        before,
        "POISON: a signature-OVERFLOWED resolvability compute admitted its verdict into \
         ResolvabilityDb — an observation set above FACT_SIGNATURE_CAP can be rooted by no \
         signature, so the entry could never be revalidated on a warm read. Overflow must \
         refuse INDEPENDENTLY at this tracer boundary (pre-fix the `_finalise` was discarded)",
    );
}

/// PERF INVARIANT — the per-scope [`ScopeShadowing`] is built ONCE per scope and
/// memoized on the engine beside `scope_payloads`, NOT folded fresh on every
/// Pick/Omit package-root gate probe.
///
/// Discriminating identity check: two probes for the SAME scope return the SAME
/// cached `Arc<ScopeShadowing>` (`Arc::ptr_eq`). A per-field
/// `ScopeShadowing::from_scope_payload` rebuild (the pre-memo behaviour the gate
/// used to run on every published field) mints a fresh `Arc` on each call and
/// FAILS this assertion. A DIFFERENT scope gets its own distinct instance (no
/// cross-scope aliasing), and each cached shadowing observes ITS scope's
/// userland shadow names (behaviour preserved).
#[test]
fn scope_shadowing_is_built_once_per_scope_and_memoized() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    // Scope A declares a userland `Pick` (shadows the ambient builtin).
    ws.inject_file(
        "/src/A.vue".to_string(),
        Arc::from(
            "<script lang=\"ts\">\n\
             export type Pick<T, K extends keyof T> = { [P in K]: T[P] }\n\
             </script>\n<template><div /></template>",
        ),
    );
    // Scope B declares NO userland `Pick`.
    ws.inject_file(
        "/src/B.vue".to_string(),
        Arc::from(
            "<script lang=\"ts\">\n\
             export interface OtherProps { z: string }\n\
             </script>\n<template><div /></template>",
        ),
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded("/src/A.vue"));
    assert!(host.ensure_loaded("/src/B.vue"));

    let mut engine = ComponentMetaQueryEngine::new(&host);

    // Two probes for the SAME scope reuse ONE cached instance — the
    // discriminating identity check. A per-field `from_scope_payload` rebuild
    // returns two distinct `Arc`s and FAILS `ptr_eq`.
    let first = engine
        .scope_shadowing_for_scope("/src/A.vue", verter_type_expr::TopLevelOwnerId::module(0));
    let second = engine
        .scope_shadowing_for_scope("/src/A.vue", verter_type_expr::TopLevelOwnerId::module(0));
    assert!(
        Arc::ptr_eq(&first, &second),
        "the per-scope ScopeShadowing must be built ONCE and memoized — both probes \
         for the same scope must return the SAME Arc (a per-field from_scope_payload \
         rebuild fails this identity check)",
    );

    // Behaviour preserved: the cached shadowing observes scope A's userland
    // `Pick` and does NOT over-shadow an undeclared builtin.
    assert!(
        first.is_shadowing_lib("Pick"),
        "scope A's userland `type Pick` must shadow the ambient builtin",
    );
    assert!(
        !first.is_shadowing_lib("Omit"),
        "scope A declares no `Omit`, so that builtin stays unshadowed",
    );

    // A DIFFERENT scope gets its OWN distinct cached instance (no aliasing) and
    // observes ITS scope's shadow names (no userland `Pick` there).
    let other = engine
        .scope_shadowing_for_scope("/src/B.vue", verter_type_expr::TopLevelOwnerId::module(0));
    assert!(
        !Arc::ptr_eq(&first, &other),
        "distinct scopes must not alias to one shared ScopeShadowing instance",
    );
    assert!(
        !other.is_shadowing_lib("Pick"),
        "scope B declares no userland `Pick`; the builtin stays unshadowed there",
    );
}

/// Inner-cache fenced-serve poison — the declaration-lookup cache must REFUSE
/// admission when the declaration resolution it caches consumed a FENCED
/// (ReturnOnly, `store_published == false`) `IndexedReady` serve.
///
/// `resolve_type_declaration` resolves through the prepared-decl read and the
/// dep-resolution fallback, both of which ride `ensure_indexed_ready_serve`, so
/// the resolved declaration can be derived from a served-without-publication
/// (superseded) artifact. The entry it admits self-roots on
/// `authoritative_current_content_hash(canonical_source)` — the LIVE hash, read
/// BEFORE the compute — so its fact stamps validate against the live view while
/// its payload came from the superseded basis: the exact shape the read-side
/// fact rail cannot reject.
///
/// DISCRIMINATING: the unfenced control ADMITS (`live_count` grows — so the
/// fenced assertion is not vacuous); the fenced request must NOT, while the
/// declaration still resolves for THIS caller.
#[test]
fn fenced_serve_declaration_lookup_is_not_admitted() {
    use std::sync::atomic::Ordering;

    // Control — an UNFENCED declaration resolution admits its entry.
    let control = build_route_only_reexport_host();
    assert!(control.ensure_indexed_ready("/m4_src/types.ts").is_some());
    let control_db = control.project_type_store().declaration_db();
    let control_before = control_db.live_count();
    let control_resolved = {
        let mut engine = ComponentMetaQueryEngine::new(control.as_ref());
        engine
            .resolve_type_declaration(
                "/m4_src/types.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Props",
            )
            .declaration_id
            .is_some()
    };
    assert!(
        control_resolved,
        "fixture invariant: the declaration resolves on an unfenced host",
    );
    assert!(
        control_db.live_count() > control_before,
        "fixture invariant: an unfenced resolution ADMITS its declaration-lookup entry \
         (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` the resolution drives is fenced at
    // a STABLE generation (no bump, so a GenerationSuperseded gate cannot mask the
    // refusal), while the served artifact still resolves the declaration.
    let fenced = build_route_only_reexport_host();
    assert!(fenced.ensure_indexed_ready("/m4_src/types.ts").is_some());
    let fenced_db = fenced.project_type_store().declaration_db();
    let fenced_before = fenced_db.live_count();
    fenced
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);
    let fenced_resolved = {
        let mut engine = ComponentMetaQueryEngine::new(fenced.as_ref());
        engine
            .resolve_type_declaration(
                "/m4_src/types.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Props",
            )
            .declaration_id
            .is_some()
    };
    fenced
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    // The value still answers THIS caller (ReturnOnly) — refusal is CACHE-ONLY and
    // must never fabricate a miss.
    assert!(
        fenced_resolved,
        "a fenced serve refuses the CACHE WRITE, never the value: the declaration must still \
         resolve for this caller",
    );
    assert_eq!(
        fenced_db.live_count(),
        fenced_before,
        "POISON: a fenced (non-cacheable) declaration resolution admitted its entry into the \
         declaration-lookup cache. The entry self-roots on the LIVE content hash while its \
         payload came from a served-without-publication artifact, so no read-side rail can \
         reject it and a later same-generation warm read inherits the stale declaration. The \
         WHOLE compute must run inside a cacheability tracer whose verdict refuses the write",
    );
}

/// Inner-cache fenced-serve poison — [`ImportedRegistryDb`] must REFUSE
/// admission when the cross-file symbol resolution it caches consumed a FENCED
/// (ReturnOnly, `store_published == false`) `IndexedReady` serve.
///
/// `resolve_imported_registry_symbol` walks the import route and rides
/// `ensure_indexed_ready_serve`, so its resolved value can be derived from a
/// served-without-publication (superseded) artifact. The entry it admits
/// self-roots on `authoritative_current_content_hash(canonical_id)` — the LIVE
/// hash, read BEFORE the compute — so the entry's fact stamps validate against
/// the live view while its payload came from the superseded basis. That is
/// exactly the shape the read-side fact rail cannot reject, and a later
/// same-generation warm peek inherits it.
///
/// The sibling `ResolvabilityDb`, which caches the BOOL derived from this SAME
/// resolution, already traces its compute and refuses. This cache — which caches
/// the resolution's VALUE — did not.
///
/// DISCRIMINATING: the unfenced control ADMITS (`live_count` grows — so the
/// fenced assertion is not vacuous); the fenced request must NOT, while the
/// symbol still resolves for THIS caller (refusal is cache-only, never a
/// fabricated miss).
#[test]
fn fenced_serve_imported_registry_symbol_is_not_admitted() {
    use std::sync::atomic::Ordering;

    // Control — an UNFENCED resolution admits its entry.
    let control = build_route_only_reexport_host();
    assert!(control.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let control_db = control.project_type_store().imported_registry_db();
    let control_before = control_db.live_count();
    let control_resolved = {
        let mut engine = ComponentMetaQueryEngine::new(control.as_ref());
        engine
            .resolve_imported_registry_symbol(
                "/m4_src/index.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "ButtonProps",
            )
            .is_some()
    };
    assert!(
        control_resolved,
        "fixture invariant: the re-exported symbol resolves on an unfenced host",
    );
    assert!(
        control_db.live_count() > control_before,
        "fixture invariant: an unfenced resolution ADMITS its ImportedRegistryDb entry \
         (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` the route walk drives is fenced
    // at a STABLE generation (no bump, so a GenerationSuperseded gate cannot mask
    // the refusal), while the served artifact still resolves the symbol.
    let fenced = build_route_only_reexport_host();
    assert!(fenced.ensure_indexed_ready("/m4_src/index.ts").is_some());
    let fenced_db = fenced.project_type_store().imported_registry_db();
    let fenced_before = fenced_db.live_count();
    fenced
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);
    let fenced_resolved = {
        let mut engine = ComponentMetaQueryEngine::new(fenced.as_ref());
        engine
            .resolve_imported_registry_symbol(
                "/m4_src/index.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "ButtonProps",
            )
            .is_some()
    };
    fenced
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    // The value still answers THIS caller (ReturnOnly) — refusal is CACHE-ONLY and
    // must never fabricate a miss.
    assert!(
        fenced_resolved,
        "a fenced serve refuses the CACHE WRITE, never the value: the symbol must still \
         resolve for this caller",
    );
    assert_eq!(
        fenced_db.live_count(),
        fenced_before,
        "POISON: a fenced (non-cacheable) cross-file symbol resolution admitted its entry into \
         ImportedRegistryDb. The entry self-roots on the LIVE content hash while its payload \
         came from a served-without-publication artifact, so no read-side rail can reject it \
         and a later same-generation warm peek inherits the stale resolution. The WHOLE compute \
         must run inside a cacheability tracer whose verdict downgrades the admission to \
         ReturnOnly — the same rail the sibling ResolvabilityDb (which caches the BOOL derived \
         from this SAME resolution) already carries",
    );
}

/// A two-declaration owner: `Pin` exists so a test can acquire the file's
/// retained decl-body lease with a demand that is NOT the one under test
/// (`Collection`), leaving `Collection`'s write-once prepared-decl slot VACANT.
fn build_owner_collection_host() -> Arc<VerterHost> {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/oc_src/types.ts".to_string(),
        Arc::from(
            "export interface Pin { p: number }\n\
             export interface Collection { primary: string }\n",
        ),
    );
    let host = Arc::new(VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    ));
    assert!(host.ensure_loaded("/oc_src/types.ts"));
    host
}

/// Inner-cache LEASE-MISS poison — [`OwnerCollectionDb`] must REFUSE admission
/// when the prepared-decl read it caches consumed a BROKEN DECL-BODY LEASE.
///
/// This is a CONTENT-NEUTRAL non-cacheable reason, and that is the whole point.
/// A `LeaseMiss` does not supersede the artifact and does not move the owner's
/// content hash: the file stays published and content-current. So the cached
/// `None` locator self-roots on the LIVE hash, its fact stamps validate against
/// the live view on every subsequent warm read, and NO read-side rail can ever
/// reject it — the recoverable declaration is shadowed as a permanent absence at
/// that content version. "Safe by rooting" is a category error here.
///
/// `PreparedDeclBundle::get` deliberately leaves its write-once slot VACANT on a
/// `LeaseMiss` precisely so a later demand under a live lease recovers; the
/// `OwnerCollectionDb` one level up must not undo that care by publishing the
/// resulting `None` with a valid root.
///
/// The lease miss happens inside `observed_prepared_type_decl`, which runs
/// BEFORE the cache funnel's compute closure — so a cacheability scope opened
/// INSIDE the closure would miss it. The scope has to be the outermost bracket
/// of the producer.
///
/// DISCRIMINATING: the live-lease control ADMITS (`live_count` grows, locator is
/// `Some` — so the broken-lease assertion is not vacuous); the broken-lease
/// request must NOT admit.
#[test]
fn broken_decl_body_lease_owner_collection_is_not_admitted() {
    // Control — a LIVE-lease owner-collection read admits its entry.
    let control = build_owner_collection_host();
    let control_db = control.project_type_store().owner_collection_db();
    let control_before = control_db.live_count();
    let control_locator = {
        let mut engine = ComponentMetaQueryEngine::new(control.as_ref());
        engine.owner_collection_expr(
            "/oc_src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Collection",
        )
    };
    assert!(
        control_locator.is_some(),
        "fixture invariant: the collection locator resolves under a live decl-body lease",
    );
    assert!(
        control_db.live_count() > control_before,
        "fixture invariant: a live-lease read ADMITS its OwnerCollectionDb entry \
         (otherwise the broken-lease assertion is vacuous)",
    );

    // Broken lease — pin the owner's retained parse snapshot with a demand for a
    // DIFFERENT symbol (`Pin`), then release it out-of-band. `Collection`'s
    // prepared-decl slot is still VACANT, so its next demand lease-misses while
    // the artifact stays published and content-current (no hash movement).
    let broken = build_owner_collection_host();
    let serve = broken
        .ensure_indexed_ready_serve("/oc_src/types.ts")
        .expect("the owner indexes");
    assert!(
        serve.store_published,
        "fixture invariant: the artifact is PUBLISHED and content-current — the poison this \
         test pins is content-NEUTRAL, so a fenced serve must not be what refuses it",
    );
    let state = Arc::clone(&serve.indexed.shallow_state);
    assert!(
        state.decl_bodies().type_decl("Pin").is_some(),
        "fixture invariant: the pin demand must acquire the owner's retained-snapshot lease",
    );
    state.decl_bodies().release_retained_snapshot_for_test();

    let broken_db = broken.project_type_store().owner_collection_db();
    let broken_before = broken_db.live_count();
    let broken_locator = {
        let mut engine = ComponentMetaQueryEngine::new(broken.as_ref());
        engine.owner_collection_expr(
            "/oc_src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Collection",
        )
    };
    assert!(
        broken_locator.is_none(),
        "fixture invariant: the broken lease must actually produce the degraded `None` locator \
         — otherwise this test observes no poison to refuse",
    );
    assert_eq!(
        broken_db.live_count(),
        broken_before,
        "POISON: a broken-lease (`LeaseMiss`) prepared-decl read admitted its degraded `None` \
         locator into OwnerCollectionDb. The lease miss is CONTENT-NEUTRAL — the owner stays \
         published and content-current — so the entry roots on the LIVE hash and validates on \
         every warm read forever, permanently shadowing a recoverable declaration. The whole \
         producer (INCLUDING the `observed_prepared_type_decl` read, which is where the lease \
         miss is consumed) must run inside a cacheability tracer whose verdict refuses the write",
    );
}

/// Engine SCRATCH-memo LEASE-MISS shadow — the per-request
/// `prepared_type_decls` memo must NOT persist a degraded `None`.
///
/// `ComponentMetaQueryEngine::prepared_type_decl` memoizes its lookup for the
/// engine's whole lifetime. That `None` has TWO indistinguishable causes: an
/// honest absence (no such declaration), and a broken decl-body lease
/// (`PreparedDeclBundle::get` fans `LeaseMiss` and returns `None`). Every layer
/// BELOW deliberately keeps the degraded miss RECOVERABLE — the prepared-decl
/// bundle leaves its write-once slot VACANT, the decl-body memo evicts its
/// poisoned cell — precisely so a later demand under a live lease recovers.
/// A scratch memo that persists the degraded `None` undoes that care for the
/// engine's whole scope: the recoverable declaration is shadowed as a permanent
/// absence for every subsequent lookup in the same request.
///
/// DISCRIMINATING: the CONTROL (a fresh engine after the same recovery) proves
/// the host really did recover the declaration, so the second-lookup assertion
/// is not vacuous; the shadowed engine must reach the SAME declaration.
#[test]
fn broken_decl_body_lease_prepared_decl_scratch_memo_does_not_shadow_recovery() {
    let host = build_owner_collection_host();

    // Break the owner's decl-body lease: pin the retained parse snapshot with a
    // demand for a DIFFERENT symbol (`Pin`), then release it out-of-band.
    // `Collection`'s prepared-decl slot is still VACANT, so its next demand
    // lease-misses while the artifact stays published and content-current.
    let serve = host
        .ensure_indexed_ready_serve("/oc_src/types.ts")
        .expect("the owner indexes");
    assert!(
        serve.store_published,
        "fixture invariant: the artifact is PUBLISHED and content-current — the shadow this \
         test pins is content-NEUTRAL",
    );
    let state = Arc::clone(&serve.indexed.shallow_state);
    assert!(
        state.decl_bodies().type_decl("Pin").is_some(),
        "fixture invariant: the pin demand must acquire the owner's retained-snapshot lease",
    );
    state.decl_bodies().release_retained_snapshot_for_test();

    let mut engine = ComponentMetaQueryEngine::new(host.as_ref());
    let degraded = engine.prepared_type_decl(
        "/oc_src/types.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Collection",
    );
    assert!(
        degraded.is_none(),
        "fixture invariant: the broken lease must actually degrade the prepared-decl read to \
         `None` — otherwise this test observes no shadow to refuse",
    );

    // Recover: a content edit republishes the owner from a FRESH parse whose
    // decl-body lease is live, so `Collection` prepares again.
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/oc_src/types.ts".to_string()),
            input_id: "/oc_src/types.ts".to_string(),
            source: Arc::from(
                "export interface Pin { p: number }\n\
             export interface Collection { primary: string; secondary: string }\n",
            ),
            file_language: crate::LanguageRegistry::global()
                .classify_static("/oc_src/types.ts")
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("the owner re-upserts");
    assert!(host.ensure_loaded("/oc_src/types.ts"));

    // CONTROL — a fresh engine (no scratch entry) reaches the recovered
    // declaration, so the host-side recovery is real.
    let control = {
        let mut fresh = ComponentMetaQueryEngine::new(host.as_ref());
        fresh.prepared_type_decl(
            "/oc_src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Collection",
        )
    };
    assert!(
        control.is_some(),
        "fixture invariant: the declaration must be recoverable after the content edit \
         (otherwise the shadow assertion below is vacuous)",
    );

    let recovered = engine.prepared_type_decl(
        "/oc_src/types.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Collection",
    );
    assert!(
        recovered.is_some(),
        "SHADOW: the engine's per-request prepared-decl scratch memo persisted a \
         LEASE-MISS-degraded `None`, so a declaration that is recoverable — and that a fresh \
         engine resolves — stays a permanent absence for the rest of this engine's scope. The \
         layers below leave the degraded miss RECOVERABLE (the prepared-decl bundle's slot stays \
         VACANT, the decl-body cell is evicted); the scratch memo must mirror them and leave its \
         slot vacant on a non-cacheable read instead of caching the degraded `None`",
    );
}
