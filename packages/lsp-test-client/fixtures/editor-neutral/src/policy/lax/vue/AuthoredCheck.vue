<script setup>
// @ts-check
/** @param {PointerEvent} event */
function handlePointer(event) {
  return event.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="handlePointer">Check</button></template>
