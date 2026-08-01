<script setup lang="ts">
defineOptions({ inheritAttrs: false })
defineProps<{ tone?: string }>()
</script>
<template><div>{{ tone }}</div></template>
