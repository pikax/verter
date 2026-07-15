//! Curated corpus representative — avatar with size enum from
//! a shared size-scale types file.

use super::harness::RequestAuditRecordAssertions;
use super::harness::{build_hermetic_host, resolve_under_audit};

const AVATAR_VUE: &str = r#"<script setup lang="ts">
import type { AvatarSize, AvatarShape } from './avatar_types';
defineProps<{ src?: string; alt?: string; size?: AvatarSize; shape?: AvatarShape }>();
</script>
<template>
  <span class="avatar" :class="[size, shape]">
    <img v-if="src" :src="src" :alt="alt" />
    <slot v-else />
  </span>
</template>
"#;

const AVATAR_TYPES_TS: &str = r#"export type AvatarSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';
export type AvatarShape = 'circle' | 'square' | 'rounded';
"#;

#[test]
fn avatar_loaded_files_exactly() {
    let host = build_hermetic_host(&[
        ("/avatar.vue", AVATAR_VUE),
        ("/avatar_types.ts", AVATAR_TYPES_TS),
    ]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/avatar.vue");
    record
        .assert_loaded_files_exactly(["/avatar.vue", "/avatar_types.ts"])
        .expect("avatar loaded-files set must match exactly");
}
