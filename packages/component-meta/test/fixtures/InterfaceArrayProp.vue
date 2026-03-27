<script setup lang="ts">
interface Book {
  title: string;
  isbn: string;
  publishedYear: number;
}

defineProps<{
  books: Book[];
}>();
</script>

<template><div /></template>
