<script setup lang="ts" generic="T extends string">
defineProps<{ name: T }>();
</script>
<template>
  <div>{{ name }}</div>
</template>
