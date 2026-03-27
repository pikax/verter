<script setup lang="ts">
defineProps<{ label: string }>();
defineOptions({ inheritAttrs: false });
</script>
<template>
  <div>{{ label }}</div>
</template>
