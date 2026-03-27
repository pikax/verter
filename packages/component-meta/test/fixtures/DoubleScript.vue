<script lang="ts">
export interface SharedProps {
  shared: boolean;
  count: number;
}
</script>
<script setup lang="ts">
defineProps<SharedProps>();
</script>
<template><div /></template>
