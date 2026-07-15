<script setup lang="ts">
type Foo = { a: string; b: number }
type Self = Pick<Self, 'a'>
defineProps<{ value: Self }>()
</script>
<template><div /></template>
