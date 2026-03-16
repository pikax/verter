<script setup lang="ts">
defineProps<{
  label: string
  onAlert?: (payload: string) => void
}>()
</script>

<template>
  <button>{{ label }}</button>
</template>
