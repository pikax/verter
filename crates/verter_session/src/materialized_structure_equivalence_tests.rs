//! Pins the OBSERVABLE output-boundary MATERIALIZED-structure contract: the
//! structure published at the output boundary equals an independently-sourced
//! structure read from the retained `EvalEnv` inventory, and a divergence
//! between them fails the assertion.
//!
//! The materialized structure produced at the output boundary
//! (`MaterializeStructureDb` → `MaterializeOutcome::Value(SemanticNodeId)`) is
//! reached publicly through `get_component_meta(canonical).props[i].type_expr`.
//! A `defineProps<Props>` where `Props<T = Item> { items?: T[] }` and `Item {
//! id: string }` materialises `items.type_expr` to a NESTED structure
//! `Array { element: Object({ id: string }) }` — a genuine materialised shape,
//! not a bare carrier. The element structure is compared against an oracle read
//! from a GENUINELY INDEPENDENT source: the retained `EvalEnv` type-symbol
//! inventory (`base_eval_env_arc(canonical).type_symbols["Item"].primary().body`,
//! a `TypeExpr`). The output-boundary side is a dispatch-driven materialisation;
//! the oracle side is a direct read of the retained, already-lowered declaration
//! body — not a second dispatch resolve that shares the materialiser's
//! `SemanticGraphStore` memo.
//!
//! Each assertion DISCRIMINATES: leaving the prop a shallow carrier (a bare
//! `Ref`, or an unresolved `IndexedAccess`, or `T[]` with `T` still unbound)
//! instead of materialising the generic-default `Item[]` fails the
//! `Array`/`Object` decomposition; materialising a DIFFERENT element shape (a
//! missing/renamed/retyped member) fails the `assert_eq!` against the
//! retained-inventory `Item` surface.
//!
//! HONESTY FLAG — `MaterializeStructureDb` has NO test-callable query: its
//! producer (`materialize_component_meta_structure`) takes a
//! `&dyn ResolverContext`, so a test cannot call it directly. The materialized
//! shape is therefore reached ONLY through `get_component_meta` props, and the
//! oracle is read from the retained `EvalEnv` inventory — NOT a second
//! materialize call, and NOT a second dispatch resolve.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_expr::{ObjectExpr, ObjectMember, TypeExpr};

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

/// The sorted `(member-name, debug-rendered member type, optional)` triples of
/// an `Object`'s direct properties — the comparable surface both the
/// materialised prop element and the retained-inventory oracle reduce to.
///
/// Property-only by design: index signatures, call signatures, and spreads are
/// intentionally out of scope for these `{ id: string }`-style property
/// fixtures and are filtered out; if a fixture grew such members this helper
/// would need widening.
fn object_surface(shape: &ObjectExpr) -> Vec<(String, String, bool)> {
    let mut members: Vec<(String, String, bool)> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => {
                Some((prop.name.clone(), format!("{:?}", prop.ty), prop.optional))
            }
            _ => None,
        })
        .collect();
    members.sort();
    members
}

/// A `defineProps<Props>` with a generic-default array prop materialises the
/// prop's `type_expr` to a nested `Array { element: Object(...) }` at the output
/// boundary, and that materialised element structure equals the `Item` surface
/// read from the retained `EvalEnv` inventory.
///
/// Discriminating: (1) the `Array`-then-`Object` decomposition reds if the prop
/// stayed a shallow carrier — the explicit anti-`Ref` / anti-`IndexedAccess`
/// assertions catch a published `Ref { name: "Props" }` / an unresolved
/// `Item[]` / an unbound `T[]` instead of the materialised shape. (2) The
/// `assert_eq!` of the materialised element surface against the SECOND-SOURCE
/// retained-inventory `Item` body (`{ id: Primitive(String), optional=false }`)
/// reds if the materialiser produced a divergent element — a missing/renamed
/// member, a wrong member type, or a wrong optionality. The oracle is read from
/// the retained `EvalEnv` type-symbol inventory (a direct read of the
/// already-lowered declaration body), NOT a dispatch resolve that shares the
/// materialiser's `SemanticGraphStore` memo — so the equality is genuinely
/// independent agreement, not a same-memo tautology.
#[test]
fn materialized_generic_default_prop_structure_matches_oracle() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script setup lang="ts">
interface Item { id: string }
interface Props<T = Item> { items?: T[] }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // INDEPENDENT oracle: read `Item`'s already-lowered body straight from the
    // retained `EvalEnv` type-symbol inventory (the `<script setup>` interface
    // lands in the script's type inventory). This is the structure the omitted
    // generic-default `T = Item` must materialise into the array element —
    // sourced from the retained inventory, NOT re-resolved through the dispatch
    // that backs the materialiser (so it does not share the materialiser's
    // `SemanticGraphStore` memo).
    let item_env = project
        .host()
        .base_eval_env_arc("/Generic.vue")
        .expect("the retained eval env for /Generic.vue must build");
    let item_headers = &item_env
        .type_symbols
        .get("Item")
        .expect("the `<script setup>` interface `Item` must land in the retained type_symbols")
        .primary()
        .direct_member_headers;
    let oracle_surface: Vec<(String, bool)> = item_headers
        .iter()
        .map(|header| (header.name.clone(), header.optional))
        .collect();
    assert_eq!(
        oracle_surface,
        vec![("id".to_string(), false)],
        "the INDEPENDENT retained-inventory oracle `Item` must carry exactly the \
         non-optional `id` member header; got {oracle_surface:?}"
    );

    // Output boundary: the materialised prop `type_expr`.
    let meta = get_meta(&project, "/Generic.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(prop_names, vec!["items"], "exactly one prop `items`");
    let items = meta
        .props
        .iter()
        .find(|p| p.name == "items")
        .expect("prop `items` must exist");
    assert!(
        !items.required,
        "the optional `items?` must publish as not-required"
    );

    // The materialised structure must be a NESTED `Array { element: Object }` —
    // NOT a shallow carrier. The anti-carrier arms make "materialised" earned.
    let items_type = crate::test_only::semantic_source_probe::demand_type_expr(
        project.host(),
        "/Generic.vue",
        items
            .type_source
            .present()
            .expect("prop `items` must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("`items`'s published source must demand-materialize"));
    assert!(
        !matches!(&items_type, TypeExpr::Ref { .. }),
        "the materialised `items` must NOT be a shallow bare `Ref` carrier; got {items_type:?}"
    );
    let TypeExpr::Array { element, .. } = &items_type else {
        panic!(
            "the generic-default `items?: T[]` must materialise to an Array structure, not a \
             shallow carrier; got {items_type:?}"
        );
    };
    assert!(
        !matches!(element.as_ref(), TypeExpr::IndexedAccess { .. }),
        "the materialised array element must be the RESOLVED structure, not an unresolved \
         IndexedAccess carrier; got {element:?}"
    );
    assert!(
        !matches!(
            element.as_ref(),
            TypeExpr::TypeParameter(_) | TypeExpr::Ref { .. }
        ),
        "the materialised array element must be `Item`'s resolved Object body, not the unbound \
         generic `T`/a bare `Ref`; got {element:?}"
    );
    let TypeExpr::Object(materialized_shape) = element.as_ref() else {
        panic!(
            "the omitted generic default `T = Item` must materialise the array element to Item's \
             Object body; got {element:?}"
        );
    };

    // Equivalence: the materialised element surface equals the retained-inventory
    // `Item` surface — full structural decomposition (member name + type +
    // optionality), so a divergence in any member is caught. The two sides are
    // genuinely independent: a dispatch-driven output-boundary materialisation vs
    // a direct read of the retained, already-lowered `EvalEnv` declaration body.
    let materialized_surface = object_surface(materialized_shape);
    assert_eq!(
        materialized_surface,
        vec![("id".to_string(), "Primitive(String)".to_string(), false)],
        "the materialised array element must be exactly {{ id: string }} (non-optional); \
         got {materialized_surface:?}"
    );
    let materialized_headers: Vec<(String, bool)> = materialized_surface
        .iter()
        .map(|(name, _, optional)| (name.clone(), *optional))
        .collect();
    assert_eq!(
        materialized_headers, oracle_surface,
        "the materialised array element structure must agree with the retained-inventory \
         `Item` member headers; materialised={materialized_headers:?} oracle={oracle_surface:?}"
    );
}
