<script setup lang="ts">
withDefaults(defineProps<{ foo?: string }>(), {
  foo: "test",
});
</script>
<template>
  <span>1</span>
</template>
