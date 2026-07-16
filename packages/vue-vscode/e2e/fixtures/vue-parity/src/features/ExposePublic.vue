<script setup lang="ts">
import { ref } from "vue";

/** Public via defineExpose */
const publicCount = ref(0);
/** Internal only — must NOT appear on the public component type for non-test importers */
const secretInternal = ref("secret");

defineExpose({ publicCount });
</script>

<template>
  <div>{{ publicCount }}</div>
</template>
