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
//! function-type aliases, each published as a shallow Ref — this fixture pins
//! Shallow-By-Default publication + import-route / read-set membership, NOT
//! member-type resolution), a PATH-PROJECTED imported member that DOES force
//! cross-file member-TYPE resolution (`Foo['bar']` publishes the resolved
//! terminal `number` — impossible without resolving the imported body), a
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

/// What this test pins (its TRUE axis): Component-Meta Shallow-By-Default
/// publication PLUS import-route / dep-signature read-set membership — NOT cross-file
/// member-TYPE resolution.
///
/// A `defineProps<{ … }>` whose members are typed by IMPORTED alias names
/// publishes each member's type as a SHALLOW `Ref` carrier derived from the
/// SFC's own inline macro syntax: `label` as `Ref { name: "Label" }` (NOT the
/// inlined `string` primitive), `onSubmit` as `Ref { name: "Submit" }` (NOT
/// the inlined function body), and `item` as `Ref { name: "Foo" }` (NOT Foo's
/// inlined `Object` body). This is the Component-Meta Shallow-By-Default
/// contract: imported alias names are NOT eagerly inlined at the publication
/// surface; the consumer re-resolves `Foo` / `Label` / `Submit` through the
/// registry on demand. The full surface is pinned: the exact member set and
/// required-ness derived from optionality.
///
/// The imported-alias member rides the publication boundary directly (the
/// macro type arg is the inline object literal), where the shallow-by-default
/// `BareCarrier` rule holds for an imported alias — exactly the
/// `published_bare_alias_ref_stays_shallow` contract observed end-to-end
/// through the equivalence net.
///
/// Why this fixture CANNOT prove cross-file TYPE resolution: because the
/// members deliberately stay bare `Ref`s, the published member surface is
/// fully derivable from `/component.vue`'s OWN inline object literal — the
/// imported carriers' BODIES (`Foo`'s `{ bar }`, `Label`'s `string`, etc.)
/// never enter the published surface. A bare `Ref { name: "Foo" }` is EXACTLY
/// what `/component.vue` would publish in isolation, and is identical whether
/// or not `Foo`'s type body was resolved. The genuine cross-file
/// member-TYPE-resolution discriminator (a PATH-PROJECTED imported member
/// whose published value can ONLY come from resolving the imported type's
/// body) lives in the sibling
/// `cross_file_imported_type_resolves_through_path_projection` below.
///
/// What the dep-signature DOES discriminate here: import-route / SPECIFIER
/// resolution recording the carrier in the published read-set. The published
/// read-set's dep-signature MUST include the imported carriers `/foo.ts` and
/// `/types.ts` — the import specifiers were route-resolved and each carrier
/// canonical entered the published read-set. This proves only read-set
/// membership; it does NOT prove the carrier files were semantically consulted
/// (their symbols/bodies resolved) — an implementation that route-resolves the
/// specifiers and records the canonical deps but never resolves the carrier
/// symbols/bodies would still record the same dep-signature. A regression that
/// broke the import ROUTE would drop the carrier from the read-set (the shallow
/// `Ref` asserts alone could not tell a route-resolved import from a broken one
/// — both leave the bare name). The dep-signature asserts close that ROUTE gap;
/// they do NOT prove the member TYPE resolved (the
/// `cross_file_imported_type_resolves_through_path_projection` sibling does).
///
/// Discriminating, on three SEPARATE axes:
///
/// (1) Local macro surface + Shallow-By-Default: the
/// `["item", "label", "onSubmit"]` member set, the shallow-`Ref` arms, and the
/// not-required asserts pin the LOCAL macro surface and the Shallow-By-Default
/// publication. Every one of those facts — the three names and their
/// optionality — is derivable ENTIRELY from `/component.vue`'s OWN inline
/// `defineProps<{ ... }>` object literal. These asserts therefore do NOT
/// discriminate a broken import route: a broken route would STILL publish those
/// same three optional bare `Ref`s (this holds for the inline-object-literal
/// macro shape this fixture uses; a `defineProps<DirectAlias>()` over a broken
/// import instead drops to an empty surface).
///
/// (2) Import-route discrimination: ONLY the DEP-SIGNATURE asserts catch a
/// broken import route. A regression that broke the route drops `/foo.ts` /
/// `/types.ts` from the published read-set, and the
/// `any(... == "/foo.ts")` / `any(... == "/types.ts")` dep-signature asserts
/// fail.
///
/// (3) Eager-inline regression: the anti-`Object` and shallow-`Ref` arms catch
/// an EAGER-INLINE producer flip. If the producer flip EAGERLY inlined the
/// imported alias `Foo` at the member surface, `item`'s `type_expr` would be
/// `Foo`'s `Object` body (or its resolved alias) instead of the bare
/// `Ref { name: "Foo" }` and the shallow-`Ref` assert (plus its explicit
/// anti-`Object` arm) fails. `label` and `onSubmit` are pinned the same way: an
/// eager inline to the `string` primitive or the function body fails their
/// shallow-`Ref` arms.
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
        "the inline macro prop surface must publish exactly the three author-written members"
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

    // Import-route / specifier-resolution PROOF (the ROUTE discriminator the
    // shallow `Ref`s above cannot provide on their own — NOT a member-TYPE
    // resolution proof; for that see the path-projection sibling below): the
    // published read-set MUST root on the imported carriers. `Foo` is defined
    // in `/foo.ts`; `Label`/`Submit` in `/types.ts`. Both must appear in the
    // published component-meta entry's dep-signature because the SFC's import
    // specifiers were route-resolved and the carrier canonicals recorded in the
    // published read-set.
    //
    // SCOPE OF THIS ASSERT (what it proves and what it does NOT): it proves the
    // import ROUTE / specifier resolved and the carrier entered the read-set.
    // It does NOT prove the imported TYPE BODY resolved into the member surface
    // — a carrier enters the dep-signature on SPECIFIER resolution (route /
    // specifier resolution recording the carrier in the read-set), independent
    // of whether the member's type body was ever resolved. The
    // member-TYPE-resolution discriminator is the
    // `cross_file_imported_type_resolves_through_path_projection` sibling, where
    // the published value is `number`/`string` — IMPOSSIBLE without resolving
    // the imported body.
    //
    // NEGATIVE CONTROL (route-level): a bare `Ref { name: "Foo" }` is EXACTLY
    // what `/component.vue` publishes in ISOLATION when the import route to
    // `/foo.ts` is broken — an unresolved import leaves the bare name as a
    // `Ref`, so every shallow-`Ref` assertion above would STILL PASS against a
    // regression that broke the import ROUTE. These dep-signature asserts close
    // that ROUTE gap: if the import route to `/foo.ts` (or `/types.ts`) were
    // removed/broken, the carrier would NOT enter the read-set, and the
    // corresponding `any(... == "/foo.ts")` assert FAILS.
    // (Empirically the dep-signature for this fixture is
    // `["/component.vue", "/foo.ts", "/types.ts"]`.)
    let dep_canonicals =
        crate::component_meta_result_db::ComponentMetaResultDb::dep_signature_for_owner_in_test(
            project.host(),
            "/component.vue",
        );
    assert!(
        dep_canonicals.iter().any(|c| c.as_ref() == "/foo.ts"),
        "import-route resolution must record the `Foo` carrier `/foo.ts` in the \
         published read-set; a broken import route to `/foo.ts` would leave the \
         same bare `Ref {{ name: \"Foo\" }}` yet drop `/foo.ts` from the dep-signature. \
         observed {dep_canonicals:?}"
    );
    assert!(
        dep_canonicals.iter().any(|c| c.as_ref() == "/types.ts"),
        "import-route resolution must record the `Label`/`Submit` carrier `/types.ts` \
         in the published read-set; a broken import route to `/types.ts` \
         would leave the same bare `Label`/`Submit` refs yet drop `/types.ts` from the \
         dep-signature. observed {dep_canonicals:?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// Cross-file imported-TYPE resolution through a PATH-PROJECTED member —
// the genuine member-TYPE-resolution discriminator (the sibling above
// proves only Shallow-By-Default publication + import-route / read-set membership).
// ════════════════════════════════════════════════════════════════════

/// A `defineProps<{ x: Foo['bar']; y?: Foo['baz'] }>` over an IMPORTED
/// interface `Foo` (defined in `/foo.ts`) publishes the path-projected
/// members as the RESOLVED terminal member types: `x` is
/// `Primitive(Number)` (the resolved `Foo.bar`) and `y` is
/// `Primitive(String)` (the resolved `Foo.baz`). Producing these values is
/// IMPOSSIBLE without resolving the imported `Foo`'s BODY cross-file — a
/// bare-`Ref` echo (the Shallow-By-Default carrier the sibling
/// `cross_file_imported_props_resolve_their_member_surface` pins for a plain
/// imported alias) CANNOT yield `number` / `string` here. Path-projection is
/// path-precise per Component-Meta Shallow-By-Default: `Foo['bar']`
/// materialises ONLY the `bar` hop's resolved type, which is exactly the
/// cross-file member-type-resolution work a declaration-body PRODUCER flip
/// must preserve. This is the discriminator the sibling's dep-signature alone
/// cannot provide (the dep-signature proves the import ROUTE / specifier
/// resolved; this test proves the imported member TYPE resolved).
///
/// Discriminating (the asserted values are impossible without cross-file
/// resolution): if cross-file member-type resolution regressed (the producer
/// flip echoed the macro member refs as shallow carriers without resolving
/// `Foo`'s body, or the indexed-access projection failed), `x` would be an
/// unresolved carrier (a bare `Ref`/`IndexedAccess`, NOT `Primitive(Number)`)
/// and the `Primitive(Number)` arm panics. The fixture deliberately uses
/// DIFFERENT primitives per key (`bar: number`, `baz: string`) so the
/// assertion discriminates the SPECIFIC resolved terminal — a regression that
/// mis-routed `x` to `baz` would land on `string` and fail.
///
/// NEGATIVE CONTROL (empirically verified by a throwaway edit, then reverted):
/// breaking `/foo.ts`'s `Foo.bar` type — e.g. removing the `bar` member, or
/// breaking the import route to `/foo.ts` — makes the published `x` an
/// unresolved carrier (NO LONGER `Primitive(Number)`), so the
/// `Primitive(Number)` assertion FAILS. The asserted value therefore genuinely
/// requires resolving the imported type's body; the test cannot pass against a
/// regression that only route-resolves imports without resolving the member
/// type. The dep-signature assert (below) additionally pins that `/foo.ts`
/// entered the read-set — so this test pins ROUTE + member-TYPE resolution
/// together.
#[test]
fn cross_file_imported_type_resolves_through_path_projection() {
    let project = make_project();
    // The interface body lives in a SEPARATE file: the only way to publish a
    // resolved `number`/`string` for the path-projected members is to resolve
    // `Foo`'s body across the file boundary. Distinct primitives per key make
    // the terminal assertion discriminate the SPECIFIC resolved hop.
    project
        .upsert_base(
            "/foo.ts",
            r#"export interface Foo { bar: number; baz: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/component.vue",
            r#"<script setup lang="ts">
import type { Foo } from './foo';
defineProps<{
  x: Foo['bar'];
  y?: Foo['baz'];
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
        vec!["x", "y"],
        "both path-projected members must publish across the file boundary"
    );
    assert!(
        prop(&meta, "x").required,
        "the non-optional `x` must publish as required"
    );
    assert!(
        !prop(&meta, "y").required,
        "the optional `y?` must publish as not-required"
    );

    // `x: Foo['bar']` MUST publish the RESOLVED terminal `number` — impossible
    // without resolving `Foo`'s body in `/foo.ts`. A bare-`Ref`/`IndexedAccess`
    // carrier (broken cross-file member-type resolution) fails this arm.
    match &prop(&meta, "x").type_expr {
        TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number) => {}
        other => panic!(
            "the path-projected imported member `x: Foo['bar']` must publish the RESOLVED \
             terminal `Primitive(Number)` (cross-file resolution of `Foo.bar`); a bare carrier \
             means cross-file member-type resolution regressed. got {other:?}"
        ),
    }
    // `y: Foo['baz']` MUST publish the RESOLVED terminal `string` — the
    // distinct primitive proves the projection routed to `baz`, not `bar`.
    match &prop(&meta, "y").type_expr {
        TypeExpr::Primitive(verter_type_expr::PrimitiveName::String) => {}
        other => panic!(
            "the path-projected imported member `y: Foo['baz']` must publish the RESOLVED \
             terminal `Primitive(String)` (cross-file resolution of `Foo.baz`); got {other:?}"
        ),
    }

    // Pin ROUTE + member-TYPE resolution together: the resolved `number`/
    // `string` above already prove `Foo`'s body resolved cross-file; this
    // additionally asserts `/foo.ts` entered the published read-set (a content
    // edit to `/foo.ts` must therefore miss the warm component-meta read).
    let dep_canonicals =
        crate::component_meta_result_db::ComponentMetaResultDb::dep_signature_for_owner_in_test(
            project.host(),
            "/component.vue",
        );
    assert!(
        dep_canonicals.iter().any(|c| c.as_ref() == "/foo.ts"),
        "cross-file member-type resolution of `Foo['bar']`/`Foo['baz']` must root the published \
         read-set on the `Foo` carrier `/foo.ts`. observed {dep_canonicals:?}"
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
