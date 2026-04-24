//! Curated corpus representative — avatar group with an array of
//! avatar-shaped props (re-uses avatar's size type).

use crate::harness::{build_hermetic_host, resolve_under_audit};

const AVATAR_GROUP_VUE: &str = r#"<script setup lang="ts">
import type { AvatarSize } from './avatar_types';
import type { AvatarGroupItem } from './avatar_group_types';
defineProps<{ items: AvatarGroupItem[]; size?: AvatarSize; max?: number }>();
</script>
<template>
  <div class="avatar-group" :class="size">
    <span v-for="(item, i) in items.slice(0, max)" :key="i">
      <img :src="item.src" :alt="item.alt" />
    </span>
  </div>
</template>
"#;

const AVATAR_GROUP_TYPES_TS: &str = r#"export interface AvatarGroupItem {
  src: string;
  alt?: string;
}
"#;

const AVATAR_TYPES_TS: &str = r#"export type AvatarSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';
"#;

#[test]
fn avatar_group_loaded_files_exactly_includes_both_type_modules() {
    let host = build_hermetic_host(&[
        ("/avatar_group.vue", AVATAR_GROUP_VUE),
        ("/avatar_group_types.ts", AVATAR_GROUP_TYPES_TS),
        ("/avatar_types.ts", AVATAR_TYPES_TS),
    ]);
    let (_analysis, _resolution, record) = resolve_under_audit(host, "/avatar_group.vue");
    record
        .assert_loaded_files_exactly([
            "/avatar_group.vue",
            "/avatar_group_types.ts",
            "/avatar_types.ts",
        ])
        .expect("avatar_group loaded-files set must match exactly (both type modules)");
}
