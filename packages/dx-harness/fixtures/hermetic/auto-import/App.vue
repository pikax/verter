<template>
  <output>{{ base }}</output>
</template>

<script setup lang="ts">
import { ref } from "vue";

const base = ref(2);

// Auto-import edit site: the scenario types a `computed(...)` usage here, and the
// auto-import completion must offer `computed` with an import edit that adds it to
// the `vue` import above. The committed base intentionally imports only `ref`.
// @dx-anchor imp.autoImportSite
</script>
