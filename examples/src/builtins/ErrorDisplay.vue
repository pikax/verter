<script setup lang="ts">
defineProps<{
  error?: Error;
}>();
</script>

<template>
  <div class="error">
    <p>Failed to load component</p>
    <p v-if="error">{{ error.message }}</p>
  </div>
</template>
