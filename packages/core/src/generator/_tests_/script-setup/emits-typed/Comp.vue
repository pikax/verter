<script setup lang="ts">
const emits = defineEmits<{
  (event: "foo", test: string): void;
}>();
</script>
<template>
  <span>1</span>
</template>
