<script setup lang="ts">
defineProps({
  label: String,
  count: Number,
  flag: Boolean,
  item: { type: Object as () => { id: number } },
})
</script>
<template><div /></template>
