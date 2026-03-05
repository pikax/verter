<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
</script>
<template><div>{{ foo }} {{ bar }}</div></template>
