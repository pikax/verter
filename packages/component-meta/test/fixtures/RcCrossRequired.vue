<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String, required: true },
  n: { type: Number, required: true },
  b: { type: Boolean, required: true },
  d: { type: Date, required: true },
  k: { type: Thing, required: true },
})
</script>
<template><div /></template>
