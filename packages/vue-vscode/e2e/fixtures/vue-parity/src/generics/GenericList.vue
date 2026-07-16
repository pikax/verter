<script setup lang="ts" generic="T extends { id: string }">
defineProps<{
  items: T[];
  selectedId?: string;
}>();
</script>

<template>
  <ul>
    <li v-for="item in items" :key="item.id">{{ item.id }}</li>
  </ul>
</template>
