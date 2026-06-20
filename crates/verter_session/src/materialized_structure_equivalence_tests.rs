//! Characterization of the OBSERVABLE output-boundary MATERIALIZED-structure
//! contract a future declaration-body PRODUCER flip (to handle-native carrier
//! bodies) must preserve, compared against an independently-resolved structure.
//!
//! The materialized structure produced at the output boundary
//! (`MaterializeStructureDb` → `MaterializeOutcome::Value(SemanticNodeId)`) is
//! reached publicly through `get_component_meta(canonical).props[i].type_expr`.
//! A `defineProps<Props>` where `Props<T = Item> { items?: T[] }` and `Item {
//! id: string }` materialises `items.type_expr` to a NESTED structure
//! `Array { element: Object({ id: string }) }` — a genuine materialised shape,
//! not a bare carrier. The element structure is compared against an
//! INDEPENDENTLY-resolved oracle: `Item` resolved on its own through the shared
//! dispatch (`resolve_named_symbol` + `project_node_to_type_expr`) — a different
//! entry point than the prop-surface materialisation, so the equality is real
//! cross-rail agreement.
//!
//! Each assertion DISCRIMINATES: a flip that left the prop a shallow carrier
//! (a bare `Ref`, or an unresolved `IndexedAccess`, or `T[]` with `T` still
//! unbound) instead of materialising the generic-default `Item[]` fails the
//! `Array`/`Object` decomposition; a flip that materialised a DIFFERENT element
//! shape (a missing/renamed/retyped member) fails the `assert_eq!` against the
//! independently-resolved `Item` surface. Written GREEN against the current
//! tree (the deep-expand materialisation already runs).
//!
//! HONESTY FLAG — `MaterializeStructureDb` has NO test-callable query: its
//! producer (`materialize_component_meta_structure`) takes a
//! `&dyn ResolverContext`, so a test cannot call it directly. The materialized
//! shape is therefore reached ONLY through `get_component_meta` props, and the
//! oracle is computed by an INDEPENDENT dispatch resolve of the underlying
//! `Item` type — NOT a second materialize call.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_expr::{ObjectExpr, ObjectMember, TypeExpr};

use crate::meta::MetaProject;
use crate::semantic_query::ProjectionMode;
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
/// materialised prop element and the independently-resolved oracle reduce to.
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
/// boundary, and that materialised element structure equals the independently
/// dispatch-resolved `Item` surface.
///
/// Discriminating: (1) the `Array`-then-`Object` decomposition reds if the prop
/// stayed a shallow carrier — the explicit anti-`Ref` / anti-`IndexedAccess`
/// assertions catch a flip that published `Ref { name: "Props" }` / an
/// unresolved `Item[]` / an unbound `T[]` instead of the materialised shape.
/// (2) The `assert_eq!` of the materialised element surface against the
/// SECOND-SOURCE `Item` resolution (`{ id: Primitive(String), optional=false }`)
/// reds if the materialiser produced a divergent element — a missing/renamed
/// member, a wrong member type, or a wrong optionality. The oracle is resolved
/// through a different entry point (a direct `resolve_named_symbol` on `Item`,
/// not the prop-surface materialisation), so the equality is genuine cross-rail
/// agreement, not a tautology.
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

    // INDEPENDENT oracle: resolve `Item` on its own through the shared dispatch
    // and project it. This is the structure the omitted generic-default `T =
    // Item` must materialise into the array element — computed via a DIFFERENT
    // entry point than the prop-surface materialisation.
    let item_node = project
        .host()
        .resolve_named_symbol("/Generic.vue", "Item", &[], Some(ProjectionMode::Expanded))
        .expect("Item must resolve Expanded");
    let item_projected = project
        .host()
        .project_node_to_type_expr(item_node)
        .expect("Item must project");
    let TypeExpr::Object(item_shape) = &item_projected else {
        panic!("the oracle `Item` must resolve to an Object body, got {item_projected:?}");
    };
    let oracle_surface = object_surface(item_shape);
    assert_eq!(
        oracle_surface,
        vec![("id".to_string(), "Primitive(String)".to_string(), false)],
        "the INDEPENDENT oracle `Item` must resolve to exactly {{ id: string }} (non-optional); \
         got {oracle_surface:?}"
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
    assert!(
        !matches!(&items.type_expr, TypeExpr::Ref { .. }),
        "the materialised `items` must NOT be a shallow bare `Ref` carrier; got {:?}",
        items.type_expr
    );
    let TypeExpr::Array { element, .. } = &items.type_expr else {
        panic!(
            "the generic-default `items?: T[]` must materialise to an Array structure, not a \
             shallow carrier; got {:?}",
            items.type_expr
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

    // Equivalence: the materialised element surface equals the INDEPENDENTLY
    // dispatch-resolved `Item` surface — full structural decomposition (member
    // name + type + optionality), so a divergence in any member is caught.
    let materialized_surface = object_surface(materialized_shape);
    assert_eq!(
        materialized_surface, oracle_surface,
        "the materialised array element structure must equal the independently dispatch-resolved \
         `Item` surface; materialised={materialized_surface:?} oracle={oracle_surface:?}"
    );
}
