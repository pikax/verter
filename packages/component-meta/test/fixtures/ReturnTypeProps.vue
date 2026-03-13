<script setup lang="ts">
function createConfig() { return { theme: 'dark' as string, debug: false } }
defineProps<{ config: ReturnType<typeof createConfig> }>()
</script>

<template><div /></template>
