//! The CORE resolved-through-dispatch equivalence contract: a declaration
//! body resolved END-TO-END through the one shared dispatch into a
//! component-meta surface produces the same public/member surface,
//! provenance flags, merge roles, alias terminals and dependency sets the
//! eager path produces TODAY. These pin the observable component-meta
//! output a future declaration-body PRODUCER flip (to handle-native carrier
//! bodies) must reproduce, compared against the retained whole-env oracle.
//!
//! Each test resolves a real `.vue`/`.ts` fixture through
//! `get_component_meta` (the public component-meta entry, the single async
//! native request) and asserts the FULL observable prop/member surface —
//! not a single field. They are GREEN against the eager path that runs
//! today and characterize the contract; the carrier-native path must agree.
//!
//! Coverage: an alias chain (published shallow), a same-file interface
//! merge surface, a cross-file import alias (published shallow), and a
//! `defineProps<T>` deep-expansion fixture.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_expr::{ObjectMember, TypeExpr};

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn get_meta(project: &Arc<MetaProject>, canonical_id: &str) -> ComponentMetaAnalysis {
    project
        .open_session_batch()
        .expect("session opens")
        .get_component_meta(canonical_id)
        .expect("get_component_meta must succeed")
        .expect("get_component_meta must return metadata")
}

fn prop_names(meta: &ComponentMetaAnalysis) -> Vec<&str> {
    meta.props.iter().map(|prop| prop.name.as_str()).collect()
}

fn prop<'m>(
    meta: &'m ComponentMetaAnalysis,
    name: &str,
) -> &'m verter_semantic::analysis::component_meta::PropAnalysis {
    meta.props
        .iter()
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| panic!("prop `{name}` must exist; got {:?}", prop_names(meta)))
}

// ════════════════════════════════════════════════════════════════════
// D2 — alias chain published shallow at the component-meta surface.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<T>` over a prop whose type is a local ALIAS CHAIN
/// (`type Outer = Inner`, `type Inner = { … }`) publishes the prop type as
/// the bare `Ref { name: "Outer" }` carrier — the alias is NOT eagerly
/// inlined at the publication surface (shallow-by-default). The full
/// surface is pinned: prop name, the shallow `Ref` type, required-ness, and
/// the author-declared provenance flag.
///
/// Discriminating: if the producer flip eagerly inlined the alias body at
/// publication, `type_expr` would be an `Object` (or the inner alias) and
/// the `Ref { name: "Outer" }` match fails. The provenance flag
/// (`declared_in_macro_type_arg`) and the `required` assert pin the
/// provenance/required surface a regression could also flip.
#[test]
fn alias_chain_prop_publishes_shallow_ref_with_author_provenance() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export type Inner = { label: string }
export type Outer = Inner
</script>
<script setup lang="ts">
defineProps<{ node: Outer }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    assert_eq!(prop_names(&meta), vec!["node"], "exactly one prop `node`");
    let node = prop(&meta, "node");

    match &node.type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Outer",
                "the alias prop must publish the bare `Outer` ref shallow, not the inner alias/body"
            );
            assert!(
                type_arguments.is_empty(),
                "no type arguments on the alias ref"
            );
        }
        other => panic!("alias chain prop must publish a shallow `Ref`, got {other:?}"),
    }
    assert!(
        node.required,
        "a non-optional defineProps member must publish as required"
    );
    assert!(
        node.declared_in_macro_type_arg,
        "the inline `defineProps<{{ node: Outer }}>` member is author-written in the macro type arg"
    );
}

// ════════════════════════════════════════════════════════════════════
// D2 — same-file interface merge surface.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<MergedProps>` over a SAME-FILE merged interface
/// (`interface MergedProps { a } + interface MergedProps { b }`) publishes
/// a surface carrying BOTH merged members — the merge unions members across
/// contributors (never last-wins, never one-contributor-only). The full
/// observable surface is pinned: exactly `{a, b}`, both required, both
/// author-declared.
///
/// Discriminating: a regressed flip that lost the merge (e.g. lowered the
/// merge as a single `Object`/`Intersection` keeping only one contributor)
/// would drop `b` (or `a`); the exact `["a", "b"]` set assert fails. This
/// is the Declaration Merging surface contract observed end-to-end.
#[test]
fn merged_interface_prop_surface_unions_both_contributors() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface MergedProps {
  /** first */
  a: number
}
export interface MergedProps {
  /** second */
  b: string
}
</script>
<script setup lang="ts">
defineProps<MergedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    let mut names = prop_names(&meta);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["a", "b"],
        "the merged interface surface must union BOTH contributors' members"
    );
    for name in ["a", "b"] {
        let member = prop(&meta, name);
        assert!(member.required, "merged member `{name}` must be required");
        assert!(
            member.declared_in_macro_type_arg,
            "merged member `{name}` is author-written through the macro type arg"
        );
    }
    let a = prop(&meta, "a");
    assert!(
        matches!(
            &a.type_expr,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "merged member `a` must keep its `number` type, got {:?}",
        a.type_expr
    );
    let b = prop(&meta, "b");
    assert!(
        matches!(
            &b.type_expr,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "merged member `b` must keep its `string` type, got {:?}",
        b.type_expr
    );
}

// ════════════════════════════════════════════════════════════════════
// D2 — cross-file import alias published shallow.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<FooProps>` where `FooProps` is IMPORTED from another file
/// publishes its members at the surface (the macro type arg is resolved
/// through the shared dispatch into the imported interface), and a member
/// whose type is itself an imported alias stays a shallow `Ref`. The full
/// surface is pinned: member names, required-ness derived from optionality,
/// and the shallow member type.
///
/// Discriminating: if the cross-file import resolution regressed, the
/// surface would be empty / wrong (`FooProps` unresolved); the
/// `["label", "onSubmit"]` set + required asserts fail. Uses the vendored
/// `cross-file-simple` fixture shape (import-type defineProps).
#[test]
fn cross_file_imported_props_resolve_their_member_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface FooProps {
  onSubmit?: (event: SubmitEvent) => void;
  label?: string;
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/component.vue",
            r#"<script setup lang="ts">
import type { FooProps } from './types';
defineProps<FooProps>();
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/component.vue");
    let mut names = prop_names(&meta);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["label", "onSubmit"],
        "the imported FooProps surface must resolve both members across the file boundary"
    );
    assert!(
        !prop(&meta, "label").required,
        "the optional imported `label?` must publish as not-required"
    );
    assert!(
        !prop(&meta, "onSubmit").required,
        "the optional imported `onSubmit?` must publish as not-required"
    );
    assert!(
        matches!(
            &prop(&meta, "label").type_expr,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "imported `label` keeps its `string` type, got {:?}",
        prop(&meta, "label").type_expr
    );
}

// ════════════════════════════════════════════════════════════════════
// D2 — defineProps<T> with a generic default: deep expansion.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<Props>` where `Props<T = Item>` and the macro omits the
/// generic argument deep-expands the omitted default `Item` into the
/// member surface: `items?: T[]` materialises to `Item[]` exposing `Item`'s
/// `id` member. This pins the cross-declaration generic-default expansion
/// the dispatch performs for component-meta deep expansion.
///
/// Discriminating: if the generic-default substitution regressed (left `T`
/// unbound or failed to instantiate `Item`), the `items` element would not
/// be `Item`'s `Object` body and the `id`-member assert fails. (Mirrors the
/// existing default-type-parameter contract, resolved end-to-end.)
#[test]
fn generic_default_props_deep_expand_into_member_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<T = Item> {
  items?: T[]
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Generic.vue");
    assert_eq!(prop_names(&meta), vec!["items"], "exactly one prop `items`");
    let items = prop(&meta, "items");
    assert!(
        !items.required,
        "the optional `items?` must publish as not-required"
    );

    let TypeExpr::Array { element, .. } = &items.type_expr else {
        panic!(
            "`items` must resolve to an array, got {:?}",
            items.type_expr
        );
    };
    let TypeExpr::Object(shape) = element.as_ref() else {
        panic!(
            "the omitted generic default must instantiate to Item's Object body, got {element:?}"
        );
    };
    assert!(
        shape.properties.iter().any(|member| matches!(
            member,
            ObjectMember::Property(prop) if prop.name == "id"
        )),
        "the instantiated `Item` element must expose `id`, got {:?}",
        shape.properties
    );
}
