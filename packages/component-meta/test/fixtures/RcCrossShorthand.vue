<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({ s: String, n: Number, b: Boolean, d: Date, k: Thing })
</script>
<template><div /></template>
