<script setup>
// @ts-expect-error checked JavaScript keeps unannotated named-handler parameters as any
function handlePointer(domEvent) {
  const horizontal = domEvent.clientX;
  const pointerKind = domEvent.pointerType;
  return `${pointerKind}:${horizontal}`;
}
</script>

<template>
  <button type="button" @click="handlePointer">inspect pointer</button>
</template>
