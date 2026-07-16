<script setup lang="ts">
defineOptions({ inheritAttrs: false });
defineProps<{ label: string }>();
</script>

<template>
  <button>{{ label }}</button>
</template>
