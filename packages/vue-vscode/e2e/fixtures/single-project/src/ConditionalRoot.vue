<script setup lang="ts">
defineProps<{ show: boolean; text: string }>();
</script>
<template>
  <div v-if="show">{{ text }}</div>
  <div v-else>fallback</div>
</template>
