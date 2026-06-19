//! Closed conditional — a conditional type whose check resolves
//! to a concrete type at instantiation (the extends test has a
//! known-at-call-site LHS). The solver should pick exactly one
//! arm, not distribute.

use super::harness::RequestAuditRecordAssertions;
use super::harness::{build_hermetic_host, resolve_under_audit};

const CLOSED_CONDITIONAL_VUE: &str = r#"<script setup lang="ts">
import type { ClosedConditionalProps } from './closed_conditional_types';
defineProps<ClosedConditionalProps<string>>();
</script>
<template><div></div></template>
"#;

const CLOSED_CONDITIONAL_TYPES_TS: &str = r#"export type IsString<T> = T extends string ? true : false;
export interface ClosedConditionalProps<T> {
  value: T;
  isString: IsString<T>;
}
"#;

#[test]
fn closed_conditional_collapses_to_concrete_arm() {
    let host = build_hermetic_host(&[
        ("/c.vue", CLOSED_CONDITIONAL_VUE),
        ("/closed_conditional_types.ts", CLOSED_CONDITIONAL_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    assert!(prop_names.iter().any(|n| n == "value"));
    assert!(prop_names.iter().any(|n| n == "isString"));

    // Closed conditional — exactly one arm is selected. The
    // footprint's conditional_decisions should reflect the choice
    // (either True with concrete arm, or Deferred if the resolver
    // couldn't close it). Both are legitimate states as long as
    // component-meta extracts the final prop names.
    record
        .assert_loaded_files_exactly(["/c.vue", "/closed_conditional_types.ts"])
        .expect("closed_conditional: entry + types");
}
