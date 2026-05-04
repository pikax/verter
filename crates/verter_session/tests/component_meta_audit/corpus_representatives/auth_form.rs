//! Curated corpus representative — auth form with cross-file
//! field-config types.

use crate::harness::RequestAuditRecordAssertions;
use crate::harness::{build_hermetic_host, resolve_under_audit};

const AUTH_FORM_VUE: &str = r#"<script setup lang="ts">
import type { AuthFormField, AuthFormSubmit } from './auth_form_types';
defineProps<{ fields: AuthFormField[]; submit: AuthFormSubmit }>();
defineEmits<{ submit: [value: Record<string, string>] }>();
</script>
<template>
  <form @submit.prevent="$emit('submit', {})">
    <div v-for="f in fields" :key="f.name">
      <label>{{ f.label }}</label>
      <input :type="f.type" :name="f.name" :required="f.required" />
    </div>
    <button type="submit">{{ submit.label }}</button>
  </form>
</template>
"#;

const AUTH_FORM_TYPES_TS: &str = r#"export interface AuthFormField {
  name: string;
  label: string;
  type: 'text' | 'email' | 'password';
  required?: boolean;
}
export interface AuthFormSubmit {
  label: string;
  loading?: boolean;
}
"#;

#[test]
fn auth_form_loaded_files_exactly() {
    let host = build_hermetic_host(&[
        ("/auth_form.vue", AUTH_FORM_VUE),
        ("/auth_form_types.ts", AUTH_FORM_TYPES_TS),
    ]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/auth_form.vue");
    record
        .assert_loaded_files_exactly(["/auth_form.vue", "/auth_form_types.ts"])
        .expect("auth_form loaded-files set must match exactly");
}
