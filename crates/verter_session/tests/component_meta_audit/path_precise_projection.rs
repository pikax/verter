//! Path-precise projection — props typed via a deeply-indexed
//! access (`A['c']['full']['bar']`). Exercises the solver's
//! path-precise navigation rule: intermediate hops run in
//! `Navigate` mode, only the terminal hop expands.

use crate::harness::RequestAuditRecordAssertions;
use crate::harness::{build_hermetic_host, resolve_under_audit};

const PATH_PROJECTION_VUE: &str = r#"<script setup lang="ts">
import type { DeepConfig } from './deep_types';
defineProps<DeepConfig['ui']['header']>();
</script>
<template><header></header></template>
"#;

const DEEP_TYPES_TS: &str = r#"export interface DeepConfig {
  ui: {
    header: {
      title: string;
      sticky?: boolean;
    };
    footer: {
      show?: boolean;
    };
  };
  data: {
    source: string;
  };
}
"#;

#[test]
fn path_precise_projection_extracts_leaf_only() {
    let host = build_hermetic_host(&[
        ("/c.vue", PATH_PROJECTION_VUE),
        ("/deep_types.ts", DEEP_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    for required in ["title", "sticky"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "leaf path DeepConfig['ui']['header'].{required} must surface, got {prop_names:?}"
        );
    }
    // Sibling branches must NOT leak into the projection.
    for leaked in ["show", "source"] {
        assert!(
            !prop_names.iter().any(|n| n == leaked),
            "sibling `{leaked}` must NOT leak into path-precise projection, got {prop_names:?}"
        );
    }

    record
        .assert_loaded_files_exactly(["/c.vue", "/deep_types.ts"])
        .expect("path_precise_projection: entry + types");
}
