<script setup lang="ts">
import { ref } from "vue";

const exposedCount = ref(0);
function bumpExposed() {
  exposedCount.value += 1;
}
defineExpose({ exposedCount, bumpExposed });
</script>

<template>
  <span>{{ exposedCount }}</span>
</template>
