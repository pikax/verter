<script setup>
/** @param {PointerEvent} domEvent */
function handlePointer(domEvent) {
  const horizontal = domEvent.clientX;
  const pointerKind = domEvent.pointerType;
  return `${pointerKind}:${horizontal}`;
}
</script>

<template>
  <button type="button" @click="handlePointer">authored JSDoc pointer</button>
</template>
