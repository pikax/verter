<script setup lang="ts">
interface FullUser {
  id: number;
  name: string;
  email: string;
  password: string;
}
defineProps<{
  display: Pick<FullUser, "id" | "name">;
  safe: Omit<FullUser, "password">;
}>();
</script>

<template><div /></template>
