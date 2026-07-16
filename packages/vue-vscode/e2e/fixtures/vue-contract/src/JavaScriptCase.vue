<script setup>
// @ts-check
/** @type {{ label: string, count: number }} */
const jsValue = { label: "javascript", count: 2 };
/** @param {{ label: string }} value */
function renderJs(value) {
  return value.label.toUpperCase();
}
const jsRendered = renderJs(jsValue);
</script>

<template>
  <p>{{ jsRendered }} {{ jsValue.label }}</p>
</template>
