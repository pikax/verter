//! Block 6.i Transit-Shallow Publication — discriminator #3 (Identity guard).
//!
//! Regression guard: `Readonly<{ msg: string; count: number }>` MUST
//! continue to publish `msg: string` and `count: number` after the
//! Transit-Shallow Publication cutover. The Identity-mapper fast path
//! (`mapper.kind == MapperKind::Identity`, `value_expr` is structurally
//! `source[mapper_param]`) reads `source_member.value` directly in
//! `build_mapped_type`; the new
//! `mapped_surface_source_members_for_projection` helper MUST not
//! interfere with that fast path, and the Shallow walker's per-key
//! materialiser MUST honour Identity-mapper semantics when source
//! members are available.
//!
//! ## Discrimination
//!
//! - **Commit 1 (no substrate, no cutover):** PASS — HEAD already
//!   satisfies the contract.
//! - **Commit 2 (substrate added):** PASS — new helpers are additive.
//! - **Commit 3 (atomic cutover):** PASS — the cutover MUST preserve
//!   the Identity-mapper path (codex Q1 #3: Identity fast path
//!   unchanged).
//!
//! Codex's Q1 directly locks this: "Preserve `build_mapped_type`'s
//! Identity fast path untouched. The global APIs in `enumerate.rs`
//! remain for publication-grade mapped construction. They must not
//! learn `DeclRef` / `InstantiationRef`."
//!
//! If a future cutover attempt regresses Readonly/Partial/Required
//! semantics, this test fails loudly.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "component_meta_audit/harness.rs"]
mod harness;

use verter_session::audited_request::AuditedRequest;
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

// Userland MyReadonly — structurally identical to ambient `Readonly<T>`
// (both produce `MapperKind::Identity` because `value_expr` is
// structurally `source[mapper_param]`). Inlined to avoid lib-registration
// dependence; the Identity-mapper fast path is exercised the same way.
const READONLY_VUE: &str = r#"<script setup lang="ts">
interface Source {
  msg: string;
  count: number;
}
type MyReadonly<T> = { readonly [P in keyof T]: T[P] };
defineProps<MyReadonly<Source>>();
</script>
<template><div></div></template>
"#;

#[test]
fn readonly_mapped_publishes_source_member_types_unchanged() {
    let host = harness::build_hermetic_host(&[("/Readonly.vue", READONLY_VUE)]);
    let (analysis, _resolved, _audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/Readonly.vue")
        .expect("hermetic resolve must succeed");

    // Read from the unified analysis surface (same pattern as
    // `lib_parity::pick_and_my_pick_produce_identical_props`).
    let analysis_props: Vec<&verter_semantic::analysis::component_meta::PropAnalysis> =
        analysis.props.iter().collect();

    let msg = analysis_props
        .iter()
        .find(|p| p.name == "msg")
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "MyReadonly<Source> MUST publish `msg`. Props: {:?}",
                analysis_props
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let count = analysis_props
        .iter()
        .find(|p| p.name == "count")
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "MyReadonly<Source> MUST publish `count`. Props: {:?}",
                analysis_props
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
            )
        });

    // The Identity-mapper fast path means `msg` MUST end up as
    // `string` and `count` as `number` (the source member's value,
    // unchanged). A regression here points at either:
    //   (a) the Identity fast path being broken in build_mapped_type
    //   (b) the new source-surface helper unexpectedly consuming the
    //       Identity case via the per-key substrate when the source
    //       member's value should have been used directly.
    let msg_is_string = matches!(
        msg.type_expr,
        TypeExpr::Primitive(PrimitiveName::String)
            | TypeExpr::Literal(LiteralValue::String(_))
            | TypeExpr::Ref { .. },
    );
    let count_is_number = matches!(
        count.type_expr,
        TypeExpr::Primitive(PrimitiveName::Number)
            | TypeExpr::Literal(LiteralValue::Number(_))
            | TypeExpr::Ref { .. },
    );

    assert!(
        msg_is_string,
        "Block 6.i Transit-Shallow Publication — `MyReadonly<Source>.msg` MUST stay `string` \
         (Identity-mapper fast path preserved). Got type_expr: {:?}",
        msg.type_expr,
    );
    assert!(
        count_is_number,
        "Block 6.i Transit-Shallow Publication — `MyReadonly<Source>.count` MUST stay `number` \
         (Identity-mapper fast path preserved). Got type_expr: {:?}",
        count.type_expr,
    );

    // Empty publication is the most common cutover regression vector
    // (source enumeration failed → empty surface). Loud fail-fast.
    assert!(
        !analysis.props.is_empty(),
        "Block 6.i Transit-Shallow Publication — Readonly publication \
         MUST NOT empty out the props surface",
    );
}
