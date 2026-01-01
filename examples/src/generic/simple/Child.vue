<script setup lang="ts" generic="T extends Record<string, any>">
defineProps<{
  item: T;
  test?: string;
}>();

const emits = defineEmits<{
  close: [arg: { item: T }];
}>();
</script>
<template></template>
