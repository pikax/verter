//! Resolver coverage for typed slot payload bindings
//! (`defineSlots<{ default(props: { item: string }) }>`): the binding
//! parameter type must lower to `Primitive(String)`, not
//! `Unknown { raw: "semanticMiss" }`.
//!
//! Slot macros: every key of T surfaces as a slot, with bindings
//! extracted from each slot function's first parameter.
//!
//! The binding's `TypeExpr` is `Primitive(String)` for `item: string`
//! and `Primitive(Number)` for `row: number`.
//!
//! `ProjectSemanticDispatch::project_slot_binding_member` composes
//! existing variants to descend through `Function` ->
//! `params[0].ty` -> `Member(binding)`. The `expand_field_expr` closure
//! (`host_manage.rs::compute_evaluated_types*`) routes
//! `FieldKind::SlotBinding` through this helper instead of the generic
//! 2-segment `ProjectPath` (which the walker emitted `Opaque(Miss)` for
//! when reaching the slot's `Function` value with `Member(binding)`
//! remaining — `walk.rs` Function arm fall-through to `opaque_miss`).

use verter_type_expr::{PrimitiveName, TypeExpr};

use super::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

const SLOTS_TYPED_VUE: &str = r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
</script>
<template><div /></template>
"#;

/// Demand-materialize a slot binding's published type source through
/// the ONE shared dispatch — the explicit consumer resolution step for
/// a shallow-by-default publication.
fn demand_binding_type(
    host: &verter_session::VerterHost,
    owner: &str,
    binding: &verter_semantic::analysis::component_meta::SlotBindingAnalysis,
) -> TypeExpr {
    let source = binding.type_source.present().unwrap_or_else(|| {
        panic!(
            "slot binding `{}` must publish a typed source",
            binding.name
        )
    });
    verter_session::test_only::semantic_source_probe::demand_type_expr(host, owner, source)
        .unwrap_or_else(|| {
            panic!(
                "slot binding `{}`'s published source must demand-materialize",
                binding.name
            )
        })
}

#[test]
fn resolver_coverage_slot_shapes_typed_bindings_lower_to_primitive() {
    let host = build_hermetic_host_with_lib(
        &[("/c.vue", SLOTS_TYPED_VUE)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) =
        resolve_under_audit(std::sync::Arc::clone(&host), "/c.vue");

    // Both slots must be present.
    let slot_names: Vec<String> = analysis.slots.iter().map(|s| s.name.clone()).collect();
    for required in ["default", "named"] {
        assert!(
            slot_names.iter().any(|n| n == required),
            "defineSlots must surface slot `{required}`; got {slot_names:?}"
        );
    }

    // Discriminating: `default` slot's `item` binding must be
    // `Primitive(String)`. Pre-fix it is `Unknown`.
    let default_slot = analysis.slots.iter().find(|s| s.name == "default").unwrap();
    let item_binding = default_slot
        .bindings
        .iter()
        .find(|b| b.name == "item")
        .unwrap_or_else(|| {
            panic!(
                "default slot must expose binding `item`; got {:#?}",
                default_slot.bindings
            )
        });
    let item_ty = demand_binding_type(&host, "/c.vue", item_binding);
    assert_eq!(
        leaf_primitive(&item_ty),
        Some(PrimitiveName::String),
        "slot `default.item` must lower to Primitive(String); got {item_ty:#?}"
    );

    // Discriminating: `named` slot's `row` binding must be
    // `Primitive(Number)`.
    let named_slot = analysis.slots.iter().find(|s| s.name == "named").unwrap();
    let row_binding = named_slot
        .bindings
        .iter()
        .find(|b| b.name == "row")
        .unwrap_or_else(|| {
            panic!(
                "named slot must expose binding `row`; got {:#?}",
                named_slot.bindings
            )
        });
    let row_ty = demand_binding_type(&host, "/c.vue", row_binding);
    assert_eq!(
        leaf_primitive(&row_ty),
        Some(PrimitiveName::Number),
        "slot `named.row` must lower to Primitive(Number); got {row_ty:#?}"
    );

    // Negative: the `Unknown { raw: "semanticMiss" }` sentinel must
    // not appear anywhere in the slot bindings.
    for slot in &analysis.slots {
        for binding in &slot.bindings {
            let binding_ty = demand_binding_type(&host, "/c.vue", binding);
            assert!(
                !contains_unknown(&binding_ty),
                "slot binding `{}.{}` must not contain Unknown; got {binding_ty:#?}",
                slot.name,
                binding.name,
            );
        }
    }
}

/// Walk `expr` looking for a single concrete `Primitive(_)` leaf.
/// Returns `None` for `Unknown` / non-primitive shapes.
fn leaf_primitive(expr: &TypeExpr) -> Option<PrimitiveName> {
    match expr {
        TypeExpr::Primitive(p) => Some(*p),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) if arms.len() == 1 => {
            leaf_primitive(&arms[0])
        }
        _ => None,
    }
}

fn contains_unknown(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { .. } => true,
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms.iter().any(contains_unknown),
        _ => false,
    }
}
