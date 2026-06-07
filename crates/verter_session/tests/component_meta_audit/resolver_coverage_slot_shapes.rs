//! Resolver coverage for typed slot payload bindings
//! (`defineSlots<{ default(props: { item: string }) }>`): the binding
//! parameter type must lower to `Primitive(String)`, not
//! `Unknown { raw: "semanticMiss" }`.
//!
//! Source: `phase-00b-tier1-mismatches.md` row 1 (`fixture_slots_typed`).
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

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

const SLOTS_TYPED_VUE: &str = r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
</script>
<template><div /></template>
"#;

#[test]
fn resolver_coverage_slot_shapes_typed_bindings_lower_to_primitive() {
    let host = build_hermetic_host_with_lib(
        &[("/c.vue", SLOTS_TYPED_VUE)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/c.vue");

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
    assert_eq!(
        leaf_primitive(&item_binding.type_expr),
        Some(PrimitiveName::String),
        "slot `default.item` must lower to Primitive(String); got {:#?}",
        item_binding.type_expr
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
    assert_eq!(
        leaf_primitive(&row_binding.type_expr),
        Some(PrimitiveName::Number),
        "slot `named.row` must lower to Primitive(Number); got {:#?}",
        row_binding.type_expr
    );

    // Negative: the `Unknown { raw: "semanticMiss" }` sentinel must
    // not appear anywhere in the slot bindings.
    for slot in &analysis.slots {
        for binding in &slot.bindings {
            assert!(
                !contains_unknown(&binding.type_expr),
                "slot binding `{}.{}` must not contain Unknown; got {:#?}",
                slot.name,
                binding.name,
                binding.type_expr,
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
