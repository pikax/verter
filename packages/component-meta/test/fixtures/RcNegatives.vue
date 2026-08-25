<script lang="ts">
class Thing {
  id = 1
}
export { Thing }
</script>
<script setup lang="ts">
defineProps({
  thing: Thing,
  label: String,
})
</script>
<template><div /></template>
