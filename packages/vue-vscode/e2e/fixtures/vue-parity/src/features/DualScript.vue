<script lang="ts">
export const dualConstant = "dual";
</script>

<script setup lang="ts">
const dualLocal = dualConstant.toUpperCase();
</script>

<template>
  <p>{{ dualLocal }} {{ dualConstant }}</p>
</template>
