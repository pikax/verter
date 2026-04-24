//! Curated corpus representative — accordion component.
//!
//! Authored hermetic fixture modeled after a minimal accordion
//! pattern with cross-file prop types. `_exactly` asserts the
//! set of loaded files under resolution so semantic drift shows
//! up as a set-difference.

use crate::harness::{build_hermetic_host, resolve_under_audit};

const ACCORDION_VUE: &str = r#"<script setup lang="ts">
import type { AccordionItem } from './accordion_types';
defineProps<{ items: AccordionItem[]; multiple?: boolean }>();
</script>
<template>
  <div class="accordion">
    <details v-for="(item, i) in items" :key="i" :open="!multiple">
      <summary>{{ item.label }}</summary>
      <div>{{ item.content }}</div>
    </details>
  </div>
</template>
"#;

const ACCORDION_TYPES_TS: &str = r#"export interface AccordionItem {
  label: string;
  content: string;
  disabled?: boolean;
}
"#;

#[test]
fn accordion_loaded_files_exactly() {
    let host = build_hermetic_host(&[
        ("/accordion.vue", ACCORDION_VUE),
        ("/accordion_types.ts", ACCORDION_TYPES_TS),
    ]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/accordion.vue");

    // Set-match on loaded files. The entry `.vue` itself + its
    // cross-file type dep must be present (nothing more, nothing
    // less — any extra reads signal regression).
    record
        .assert_loaded_files_exactly(["/accordion.vue", "/accordion_types.ts"])
        .expect("accordion loaded-files set must match exactly");
}
