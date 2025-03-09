<script setup lang="ts">

defineProps({
  name: String,
  age: Number
})
const value = defineModel()

</script>
<template>
  <span>{{ $props }}</span>
</template>
