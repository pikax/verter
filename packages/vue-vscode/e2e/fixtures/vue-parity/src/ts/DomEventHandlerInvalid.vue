<script setup lang="ts">
function handlePointer(domEvent) {
  // @ts-expect-error PointerEvent has no notADomEventMember
  return domEvent.notADomEventMember;
}
</script>

<template>
  <button type="button" @click="handlePointer">inspect invalid pointer member</button>
</template>
