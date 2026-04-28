//! Phase 5b §5.A — TDD seed for resolver coverage gap: typed slot
//! payload bindings (`defineSlots<{ default(props: { item: string }) }>`)
//! lower the binding parameter type to `Unknown { raw: "semanticMiss" }`
//! instead of `Primitive(String)`.
//!
//! Source: `phase-00b-tier1-mismatches.md` row 1 (`fixture_slots_typed`).
//! Verter macros §slots: every key of T surfaces as a slot, with
//! bindings extracted from each slot function's first parameter.
//!
//! **Pre-Phase-5b behaviour:** the slot NAME is extracted, the binding
//! NAME is extracted, but the binding's `TypeExpr` lowers to
//! `Unknown { raw: "semanticMiss" }`.
//!
//! **Post-Phase-5b expected (after commits 2+3 — `ResolveMacroPayload`):**
//! the binding's `TypeExpr` is `Primitive(String)` for `item: string`
//! and `Primitive(Number)` for `row: number`.
//!
//! This is the ONLY seed that flips green inside Phase 5b — the
//! `ResolveMacroPayload` variant body lands in commits 2+3 and closes
//! the slot dispatch path. The other 4 seeds remain RED until
//! 5d/5e/5f.

use verter_semantic::analysis::type_expr::{PrimitiveName, TypeExpr};

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
#[ignore = "Phase 5b §5.A seed: closes IN this sub-phase at commit 2+3 via the `ResolveMacroPayload` variant body. The #[ignore] is removed in commit 2+3 once the variant lands. Verified FAIL pre-impl on commit 1."]
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
    // not appear anywhere in the slot bindings — that is the
    // pre-fix behaviour and Phase 5b closes it.
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
