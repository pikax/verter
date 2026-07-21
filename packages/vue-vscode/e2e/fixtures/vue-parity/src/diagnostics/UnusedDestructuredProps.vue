<script setup lang="ts">
const { liveOne, deadOne } = defineProps<{ liveOne: string; deadOne: number }>();
</script>

<template>
  <div>{{ liveOne }}</div>
</template>
