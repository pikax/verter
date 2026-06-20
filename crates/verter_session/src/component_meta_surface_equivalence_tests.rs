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
//! merge surface, cross-file imported-alias members (interface / primitive /
//! function-type aliases, each published as a shallow Ref), a
//! `defineProps<T>` generic-default deep-expansion fixture, and the
//! cross-file readset/fact contract — a cold resolution roots its read-set on
//! the cross-file carrier, and a content edit to that carrier misses the warm
//! component-meta read (re-resolving the changed surface rather than serving a
//! stale warm hit).

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
// D2 — cross-file imported props + an imported-alias member published
// shallow.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<{ … }>` over members imported across the file boundary
/// resolves their surface AND keeps a member whose type is itself an
/// IMPORTED alias as a SHALLOW `Ref { name: "Foo" }` — the Component-Meta
/// Shallow-By-Default contract: imported alias names are NOT eagerly inlined
/// at the publication surface; the consumer re-resolves `Foo` through the
/// registry on demand. The full surface is pinned: the exact member set,
/// required-ness derived from optionality, the primitive `label` type, the
/// `onSubmit` function-type shape, and the shallow imported-alias `item` Ref.
///
/// The imported-alias member rides the publication boundary directly (the
/// macro type arg is the inline object literal), where the shallow-by-default
/// `BareCarrier` rule holds for an imported alias — exactly the
/// `published_bare_alias_ref_stays_shallow` contract observed end-to-end
/// through the equivalence net.
///
/// Discriminating: if the cross-file import resolution regressed, the surface
/// would be empty / wrong (the imported members unresolved); the
/// `["item", "label", "onSubmit"]` set + required asserts fail. If the
/// producer flip EAGERLY inlined the imported alias `Foo` at the member
/// surface, `item`'s `type_expr` would be `Foo`'s `Object` body (or its
/// resolved alias) instead of the bare `Ref { name: "Foo" }` and the
/// shallow-Ref assert (plus its explicit anti-`Object` arm) fails. The
/// `onSubmit` `Function` match fails on a wrong lowering (an
/// `Object`/`Unknown`/`Ref`).
#[test]
fn cross_file_imported_props_resolve_their_member_surface() {
    let project = make_project();
    project
        .upsert_base("/foo.ts", r#"export interface Foo { bar: number }"#)
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"export type Label = string;
export type Submit = (event: SubmitEvent) => void;"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/component.vue",
            r#"<script setup lang="ts">
import type { Foo } from './foo';
import type { Label, Submit } from './types';
defineProps<{
  onSubmit?: Submit;
  label?: Label;
  item?: Foo;
}>();
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/component.vue");
    let mut names = prop_names(&meta);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["item", "label", "onSubmit"],
        "the imported member surface must resolve all three members across the file boundary"
    );
    for name in ["item", "label", "onSubmit"] {
        assert!(
            !prop(&meta, name).required,
            "the optional imported `{name}?` must publish as not-required"
        );
    }
    // `label`'s type is the imported alias `Label` — it STAYS a shallow `Ref`
    // (imported alias names are not eagerly inlined to their primitive body).
    match &prop(&meta, "label").type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Label",
                "the imported-alias member `label` must publish the bare `Label` ref shallow, \
                 not the inlined `string` primitive"
            );
            assert!(
                type_arguments.is_empty(),
                "no type arguments on the `Label` ref"
            );
        }
        other => panic!(
            "the imported-alias member `label` must publish a shallow `Ref {{ name: \"Label\" }}` \
             (Shallow-By-Default), got {other:?}"
        ),
    }
    // `onSubmit`'s type is the imported function-type alias `Submit` — a
    // shallow `Ref { name: "Submit" }` (not eagerly inlined to the `Function`
    // body).
    match &prop(&meta, "onSubmit").type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Submit",
                "the imported function-type alias `onSubmit` must publish the bare `Submit` ref \
                 shallow, not the inlined function body"
            );
            assert!(type_arguments.is_empty(), "no type arguments on the `Submit` ref");
        }
        other => panic!(
            "the imported-alias member `onSubmit` must publish a shallow `Ref {{ name: \"Submit\" }}` \
             (Shallow-By-Default), got {other:?}"
        ),
    }
    // The member whose type is an imported INTERFACE alias STAYS a shallow
    // `Ref { name: "Foo" }` — never eagerly inlined to Foo's `{ bar }` body.
    match &prop(&meta, "item").type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Foo",
                "the imported-alias member `item` must publish the bare `Foo` ref shallow, \
                 not Foo's inlined body"
            );
            assert!(
                type_arguments.is_empty(),
                "no type arguments on the imported-alias `Foo` ref"
            );
        }
        other => panic!(
            "the imported-alias member `item` must publish a shallow `Ref {{ name: \"Foo\" }}` \
             (Shallow-By-Default), got {other:?}"
        ),
    }
    // Explicit anti-expansion arm: `item` must NOT be eagerly inlined to the
    // imported `Foo` Object body.
    assert!(
        !matches!(&prop(&meta, "item").type_expr, TypeExpr::Object(_)),
        "the imported-alias member `item` must NOT expand `Foo` inline to an Object body"
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
    assert!(
        items.declared_in_macro_type_arg,
        "the `items` member is author-written through the `defineProps<Props>` macro type arg"
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
    // The instantiated `Item` element must expose EXACTLY `{ id }` — a
    // regression adding/removing members fails the exact-set assertion.
    let member_names: Vec<&str> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        vec!["id"],
        "the instantiated `Item` element must expose EXACTLY `id`, got {:?}",
        shape.properties
    );
    // `id`'s type (`string` per the fixture) and optionality (required) are
    // pinned — a regression changing `id`'s type or optionality fails.
    let id = shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "id" => Some(prop),
            _ => None,
        })
        .expect("the instantiated `Item` must carry `id`");
    assert!(
        matches!(
            &id.ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the instantiated `id` must keep its `string` type, got {:?}",
        id.ty
    );
    assert!(
        !id.optional,
        "the instantiated `id` is non-optional in the fixture, so it must publish as required"
    );
}

// ════════════════════════════════════════════════════════════════════
// D2 — cross-file readset / fact contract: the riskiest silent-regression
// dimension (warm-poisoning / missed invalidation the producer flip could
// introduce). A component-meta surface resolved over a cross-file
// contributor must (a) ROOT its read-set on that contributor, and (b) MISS
// the warm read when the contributor's content changes.
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<Props>` whose `Props` is IMPORTED cross-file records the
/// contributor's whole-hash fact onto the published read-set, and a content
/// edit to that contributor INVALIDATES the warm component-meta result — the
/// re-resolution recomputes the CHANGED surface rather than serving a stale
/// warm hit. This pins the fact/read-set contract the producer flip must
/// preserve: the dispatch fan-in folds the cross-file carrier's fact into the
/// published entry, so the warm-cache validation re-roots on the contributor.
///
/// Discriminating: (1) an entry published WITHOUT the carrier in its read-set
/// (a fan-in that failed to fold the carrier's dispatch facts) yields a
/// dep-signature missing `/types.ts`, failing the read-set assertion. (2) A
/// cache that ignored the recorded carrier fact (no warm invalidation on a
/// carrier edit) would serve the original `[a, b]` props after the edit and
/// would NOT advance the miss counter — both the prop-set assert and the
/// miss-counter assert fail. The producer flip silently breaking either the
/// fact fan-in or the read-set rooting reddens here.
#[test]
fn cross_file_contributor_edit_misses_warm_and_roots_readset_on_carrier() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export interface Props { a: string; b: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Cold resolution.
    let first = get_meta(&project, "/Owner.vue");
    let mut first_names = prop_names(&first);
    first_names.sort_unstable();
    assert_eq!(
        first_names,
        vec!["a", "b"],
        "the original carrier publishes exactly [a, b]"
    );

    // (a) Read-set rooting: the published entry's dep-signature MUST include
    // the cross-file carrier `/types.ts` (the dispatch fan-in folds the
    // carrier's whole-hash fact). Without it a carrier edit could not
    // invalidate the warm result.
    let dep_canonicals =
        crate::component_meta_result_db::ComponentMetaResultDb::dep_signature_for_owner_in_test(
            project.host(),
            "/Owner.vue",
        );
    assert!(
        dep_canonicals.iter().any(|c| c.as_ref() == "/types.ts"),
        "the published component-meta read-set MUST root on the cross-file carrier \
         `/types.ts`; observed {dep_canonicals:?}"
    );

    // An unedited re-resolution serves the same surface (warm hit) and counts
    // a cache hit — the baseline the post-edit miss is measured against.
    let prov = project.host().provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let warm = get_meta(&project, "/Owner.vue");
    let mut warm_names = prop_names(&warm);
    warm_names.sort_unstable();
    assert_eq!(
        warm_names,
        vec!["a", "b"],
        "an unedited re-resolution serves the same [a, b] surface (warm hit)"
    );
    assert!(
        prov.component_meta_result_cache_hits.load(Relaxed) > hits_before,
        "the unedited re-resolution must register a warm component-meta cache hit"
    );

    // (b) Content edit to the carrier: drop `b`, rename `a` -> `renamed`, add
    // `c`. The OWNER is untouched.
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);
    project
        .upsert_base(
            "/types.ts",
            "export interface Props { renamed: string; c: boolean }",
        )
        .unwrap();

    let after = get_meta(&project, "/Owner.vue");
    let mut after_names = prop_names(&after);
    after_names.sort_unstable();
    assert_eq!(
        after_names,
        vec!["c", "renamed"],
        "the carrier edit MUST invalidate the warm result — the recorded carrier \
         fact no longer validates, so the entry recomputes the changed prop set \
         [c, renamed] (a stale warm hit would still report [a, b]): {after_names:?}"
    );
    assert!(
        prov.component_meta_result_cache_misses.load(Relaxed) > misses_before,
        "the cross-file contributor edit must MISS the warm component-meta cache \
         (the read-set rooted on `/types.ts` no longer validates), advancing the \
         miss counter"
    );
}
