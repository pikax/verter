<script setup lang="ts">
// Warm-session target: the root beforeAll opens App.vue and waits for a typed
// completion on the `{{ heading }}` mustache before any suite runs.
const heading = "d1-editor-attach";
</script>

<template>
  <div>{{ heading }}</div>
</template>
