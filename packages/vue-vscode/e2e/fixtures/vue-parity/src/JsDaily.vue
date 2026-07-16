<script setup>
// @ts-check
/** @type {{ label: string, count: number }} */
const jsDaily = { label: "js", count: 2 };
/** @param {{ label: string }} value */
function renderJsDaily(value) {
  return value.label.toUpperCase();
}
const jsDailyRendered = renderJsDaily(jsDaily);
</script>

<template>
  <p>{{ jsDailyRendered }} {{ jsDaily.label }}</p>
</template>
