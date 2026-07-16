<script setup lang="ts">
defineProps<{
  count: number;
  title: string;
}>();
</script>

<template>
  <div>{{ title }}:{{ count }}</div>
</template>
