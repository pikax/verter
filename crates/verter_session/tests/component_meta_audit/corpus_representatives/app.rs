//! Curated corpus representative — root app shell with no external
//! type dependencies. Tests the single-file happy path.

use crate::harness::{build_hermetic_host, resolve_under_audit};

const APP_VUE: &str = r#"<script setup lang="ts">
defineProps<{ title: string; mode?: 'light' | 'dark' }>();
</script>
<template>
  <div class="app" :class="mode">
    <header>{{ title }}</header>
    <main><slot /></main>
  </div>
</template>
"#;

#[test]
fn app_loaded_files_exactly_single_file() {
    let host = build_hermetic_host(&[("/app.vue", APP_VUE)]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/app.vue");
    record
        .assert_loaded_files_exactly(["/app.vue"])
        .expect("single-file app.vue: only the entry file appears in loaded-files");
}
