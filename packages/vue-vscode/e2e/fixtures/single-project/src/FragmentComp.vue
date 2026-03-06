<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template>
  <div>{{ msg }}</div>
  <div>second root</div>
</template>
