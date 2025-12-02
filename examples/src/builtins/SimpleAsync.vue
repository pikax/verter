<script setup lang="ts">
// Simple async component for Suspense demo
await new Promise((resolve) => setTimeout(resolve, 1000));
</script>

<template>
  <div>Async Content Loaded!</div>
</template>
