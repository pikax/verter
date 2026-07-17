<script setup lang="ts">
interface Props {
  label: string;
}

defineProps<Props>();
const vueTsLocal = 1;
</script>

<template>
  <button :title="label">{{ vueTsLocal.toFixed(0) }}</button>
</template>
