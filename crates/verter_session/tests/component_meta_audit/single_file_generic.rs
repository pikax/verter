//! Single-file generic SFC — `<script setup generic="T">` with the
//! generic bound declared inline. Exercises the IDE's generic-SFC
//! component-meta path without cross-file dependencies.

use crate::harness::{build_hermetic_host, footprint_of, resolve_under_audit};

const SINGLE_FILE_GENERIC_VUE: &str = r#"<script setup lang="ts" generic="T extends { id: string }">
defineProps<{ rows: T[]; keyField?: keyof T }>();
defineEmits<{ select: [row: T] }>();
</script>
<template>
  <ul>
    <li v-for="row in rows" :key="row.id" @click="$emit('select', row)">
      {{ row.id }}
    </li>
  </ul>
</template>
"#;

#[test]
fn single_file_generic_loaded_files_exactly() {
    let host = build_hermetic_host(&[("/c.vue", SINGLE_FILE_GENERIC_VUE)]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");
    // Single-file: only the entry appears in loaded-files.
    record
        .assert_loaded_files_exactly(["/c.vue"])
        .expect("single-file generic: only /c.vue must appear");

    // Analysis extracts `rows` and `keyField`.
    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    assert!(prop_names.iter().any(|n| n == "rows"), "{prop_names:?}");
    assert!(prop_names.iter().any(|n| n == "keyField"), "{prop_names:?}");

    let fp = footprint_of(&record);
    assert!(!fp.indexed_ready_builds.is_empty());
}
