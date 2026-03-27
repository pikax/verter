<script setup lang="ts">
interface Base {
  id: number;
  name: string;
}
interface Extended extends Base {
  email: string;
  active?: boolean;
}
defineProps<{ user: Extended }>();
</script>

<template><div /></template>
