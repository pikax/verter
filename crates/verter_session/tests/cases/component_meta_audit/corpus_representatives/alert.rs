//! Curated corpus representative — alert component with a color
//! variant enum in a separate types file.

use super::harness::RequestAuditRecordAssertions;
use super::harness::{build_hermetic_host, resolve_under_audit};

const ALERT_VUE: &str = r#"<script setup lang="ts">
import type { AlertVariant } from './alert_types';
defineProps<{ variant?: AlertVariant; title: string; dismissible?: boolean }>();
defineEmits<{ dismiss: [] }>();
</script>
<template>
  <div class="alert" :class="variant">
    <h3>{{ title }}</h3>
    <slot />
    <button v-if="dismissible" @click="$emit('dismiss')">&times;</button>
  </div>
</template>
"#;

const ALERT_TYPES_TS: &str = r#"export type AlertVariant = 'info' | 'success' | 'warning' | 'error';
"#;

#[test]
fn alert_loaded_files_exactly() {
    let host = build_hermetic_host(&[
        ("/alert.vue", ALERT_VUE),
        ("/alert_types.ts", ALERT_TYPES_TS),
    ]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/alert.vue");
    record
        .assert_loaded_files_exactly(["/alert.vue", "/alert_types.ts"])
        .expect("alert loaded-files set must match exactly");
}
