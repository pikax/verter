<script setup lang="ts">
interface Book { title: string; isbn: string }
defineProps<{ books: Book[] }>()
</script>
<template><div /></template>
