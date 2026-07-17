<script setup>
function handlePointer(domEvent) {
  const horizontal = domEvent.clientX;
  const pointerKind = domEvent.pointerType;
  return `${pointerKind}:${horizontal}`;
}
</script>

<template>
  <button type="button" @click="handlePointer">inspect pointer</button>
</template>
