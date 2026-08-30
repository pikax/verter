<script setup>
// @ts-nocheck
/** @param {PointerEvent} event */
function handlePointer(event) {
  return event.__verterMissingPointerMember;
}
</script>

<template><button @pointerdown="handlePointer">No check</button></template>
