<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  s: { type: String },
  n: { type: Number },
  b: { type: Boolean },
  d: { type: Date },
  k: { type: Thing },
})
</script>
<template><div /></template>
