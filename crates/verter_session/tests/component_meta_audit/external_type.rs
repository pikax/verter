//! External type — props typed via an interface defined in a
//! separate `.ts` file. The canonical scenario the footprint miner
//! targets: cross-file type deps show up in `loaded_files()`.

use crate::harness::RequestAuditRecordAssertions;
use crate::harness::{build_hermetic_host, footprint_of, resolve_under_audit};

const EXTERNAL_TYPE_VUE: &str = r#"<script setup lang="ts">
import type { PanelProps } from './panel_types';
defineProps<PanelProps>();
</script>
<template><div><slot /></div></template>
"#;

const PANEL_TYPES_TS: &str = r#"export interface PanelProps {
  title: string;
  collapsible?: boolean;
  variant?: 'default' | 'compact';
}
"#;

#[test]
fn external_type_loaded_files_exactly() {
    let host = build_hermetic_host(&[
        ("/c.vue", EXTERNAL_TYPE_VUE),
        ("/panel_types.ts", PANEL_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    record
        .assert_loaded_files_exactly(["/c.vue", "/panel_types.ts"])
        .expect("external_type: entry + single type dep");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    for required in ["title", "collapsible", "variant"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "PanelProps.{required} must surface in component-meta props, got {prop_names:?}"
        );
    }

    let fp = footprint_of(&record);
    assert!(
        fp.indexed_ready_builds
            .iter()
            .any(|b| b.canonical_id.as_ref() == "/panel_types.ts"),
        "panel_types.ts must appear in indexed_ready_builds"
    );
}
