<script setup lang="ts">
// `defineProps<ImportedType>()` — props come from an IMPORTED/aliased type whose
// member declaration lives in `childImportedProps.ts` (a THIRD file), NOT an
// inline macro field. This is the canonical imported-type cross-file rename case.
import type { ChildImportedProps } from "./childImportedProps";

defineProps<ChildImportedProps>();
</script>
<template>
  <section>
    <h2>{{ headline }}</h2>
    <p v-if="subtitle">{{ subtitle }}</p>
  </section>
</template>
