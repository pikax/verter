<script setup lang="ts">
// `ref` is imported; `computed` is intentionally NOT imported, so the type
// provider offers it as an auto-import completion. Accepting it must insert
// `import { computed } from "vue"` (or merge it into the existing vue import)
// via completionItem/resolve — the provider-neutral carrier re-anchor.
import { ref } from "vue";

const base = ref(1);
const doubled = computed(() => base.value * 2);
</script>

<template>
  <div>{{ doubled }}</div>
</template>
