<script setup>
// @ts-check
/** @type {{ label: string }} */
const jsRenameValue = { label: "js-rename" };
/** @param {{ label: string }} value */
function renderJsRename(value) {
  return value.label;
}
const jsRenameOut = renderJsRename(jsRenameValue);
</script>

<template>
  <p>{{ jsRenameOut }} {{ jsRenameValue.label }}</p>
</template>
