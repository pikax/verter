<template>
  <output>{{ base }}</output>
</template>

<script setup lang="ts">
import { ref } from "vue";

const base = ref(2);

// Type `const doubled = comput` after this marker; accept `computed` auto-import.
// ANCHOR_TYPE
</script>
