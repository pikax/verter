<script setup lang="ts">
const text = "global";
</script>

<template>
  <p class="local-only">{{ text }}</p>
  <p class="leaked-global">x</p>
</template>

<style scoped>
.local-only {
  color: navy;
}
:global(.leaked-global) {
  color: crimson;
}
</style>
