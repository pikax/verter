<script setup>
function handlePointer(domEvent) {
  return domEvent.notADomEventMember;
}
</script>

<template>
  <button type="button" @click="handlePointer">inspect invalid pointer member</button>
</template>
