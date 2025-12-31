<script setup lang="ts" generic="T extends Record<string, any>">
defineProps<{
  item: T;
}>();

const emits = defineEmits<{
  close: [arg: { item: T }];
}>();
</script>
<template></template>
