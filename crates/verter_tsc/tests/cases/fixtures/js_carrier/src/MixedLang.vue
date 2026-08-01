<script lang="ts">
export const helper: number = 1
</script>
<script setup>
function bump(step) {
  return step + helper
}

const shown = bump(1)
</script>
<template>
  <div>{{ shown }}</div>
</template>
