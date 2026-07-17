<script setup>
// @ts-expect-error checked JavaScript keeps unannotated named-handler parameters as any
function handleInvalidPointer(domEvent) {
  return domEvent.notADomEventMember;
}
</script>

<template>
  <button type="button" @click="handleInvalidPointer">invalid pointer member</button>
</template>
