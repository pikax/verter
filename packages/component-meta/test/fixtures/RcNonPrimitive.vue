<script setup lang="ts">
defineProps({
  list: Array,
  obj: Object,
  fn: Function,
  sym: Symbol,
  when: Date,
  re: { type: RegExp, required: true },
  p: Promise,
})
</script>
<template><div /></template>
