<script setup lang="ts">
// Async child component stub
defineProps<{
  message?: string;
}>();
</script>

<template>
  <div class="async-child">
    <p>{{ message ?? "Async Child Component" }}</p>
  </div>
</template>
