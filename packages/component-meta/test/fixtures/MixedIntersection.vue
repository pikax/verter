<script setup lang="ts">
interface Identifiable {
  id: number;
}
type Named = { name: string; label?: string };
defineProps<Identifiable & Named & { extra: boolean }>();
</script>

<template><div /></template>
