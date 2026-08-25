<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String, default: 'md' },
  n: { type: Number, default: 0 },
  b: { type: Boolean, default: false },
  d: { type: Date, default: () => new Date() },
  k: { type: Thing, default: () => new Thing() },
})
</script>
<template><div /></template>
