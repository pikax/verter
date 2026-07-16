<script setup>
// @ts-check
/**
 * @typedef {{ title: string, count: number }} JsPropChildProps
 */
/** @type {JsPropChildProps} */
const props = defineProps({
  title: { type: String, required: true },
  count: { type: Number, required: true },
});
</script>

<template>
  <span>{{ props.title }}:{{ props.count }}</span>
</template>
