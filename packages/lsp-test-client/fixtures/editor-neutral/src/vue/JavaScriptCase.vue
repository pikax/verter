<script setup>
// @ts-check
const props = defineProps({
  label: { type: String, required: true },
});
const vueJsLocal = 1;
</script>

<template>
  <button :title="props.label">{{ vueJsLocal.toFixed(0) }}</button>
</template>
