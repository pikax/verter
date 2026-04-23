<script setup lang="ts" generic="T extends string">
defineProps<{
  value: T;
  label: string;
}>();
</script>

<template>
  <div>{{ value }} - {{ label }}</div>
</template>
