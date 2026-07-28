<script setup lang="ts">
defineProps<{ asInput: boolean }>()
</script>
<template>
  <input v-if="asInput">
  <div v-else>fallback</div>
</template>
