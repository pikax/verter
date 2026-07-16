<script setup lang="ts">
// `ref` is imported; `computed` is not — auto-import on accept.
import { ref } from "vue";

const base = ref(1);
const doubled = comput(() => base.value * 2);
</script>

<template>
  <div>{{ doubled }}</div>
</template>
