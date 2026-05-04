//! Barrel-chain — cross-file type resolution through a `types/index.ts`
//! barrel that re-exports from a leaf module. Exercises the
//! ImportTarget → canonical walk.

use crate::harness::RequestAuditRecordAssertions;
use crate::harness::{build_hermetic_host, resolve_under_audit};

const BARREL_CHAIN_VUE: &str = r#"<script setup lang="ts">
import type { DialogProps } from './barrel_index';
defineProps<DialogProps>();
</script>
<template><dialog><slot /></dialog></template>
"#;

const BARREL_INDEX_TS: &str = r#"export type { DialogProps } from './dialog_types';
"#;

const DIALOG_TYPES_TS: &str = r#"export interface DialogProps {
  open: boolean;
  title?: string;
}
"#;

#[test]
fn barrel_chain_resolves_through_reexport() {
    let host = build_hermetic_host(&[
        ("/c.vue", BARREL_CHAIN_VUE),
        ("/barrel_index.ts", BARREL_INDEX_TS),
        ("/dialog_types.ts", DIALOG_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    assert!(
        prop_names.iter().any(|n| n == "open"),
        "DialogProps.open must reach component-meta through the barrel, got {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|n| n == "title"),
        "DialogProps.title must reach component-meta through the barrel, got {prop_names:?}"
    );

    // Both the barrel and the leaf must appear in loaded-files —
    // the resolver must walk BOTH, not jump directly.
    record
        .assert_loaded_files_exactly(["/c.vue", "/barrel_index.ts", "/dialog_types.ts"])
        .expect(
            "barrel_chain loaded-files must include the barrel + leaf \
             (walker must not skip the barrel hop)",
        );
}
