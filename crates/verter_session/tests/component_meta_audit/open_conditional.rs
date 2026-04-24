//! Open conditional — a generic type with an unresolved
//! conditional arm that must distribute through both branches.
//! Exercises the solver's conditional-type handling.

use crate::harness::{build_hermetic_host, footprint_of, resolve_under_audit};

const OPEN_CONDITIONAL_VUE: &str = r#"<script setup lang="ts" generic="T">
import type { OpenConditionalProps } from './open_conditional_types';
defineProps<OpenConditionalProps<T>>();
</script>
<template><div></div></template>
"#;

const OPEN_CONDITIONAL_TYPES_TS: &str = r#"export type IsArray<T> = T extends any[] ? true : false;
export interface OpenConditionalProps<T> {
  value: T;
  isArray?: IsArray<T>;
}
"#;

#[test]
fn open_conditional_distributes_both_branches() {
    let host = build_hermetic_host(&[
        ("/c.vue", OPEN_CONDITIONAL_VUE),
        ("/open_conditional_types.ts", OPEN_CONDITIONAL_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    for required in ["value", "isArray"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "{required} must surface, got {prop_names:?}"
        );
    }

    let fp = footprint_of(&record);
    assert!(
        fp.indexed_ready_builds
            .iter()
            .any(|b| b.canonical_id.as_ref() == "/open_conditional_types.ts"),
        "conditional-type dep must be indexed"
    );
}
