<script setup lang="ts">
const knownBinding = "ok";
</script>

<template>
  <!-- intentional unresolved identifier for diagnostic coverage -->
  <p>{{ knownBinding }} {{ totallyMissingName }}</p>
</template>
